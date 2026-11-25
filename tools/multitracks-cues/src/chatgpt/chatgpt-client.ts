import {
  CONFIG,
  SELECTORS,
  launchBrowser,
  getPage,
  navigateTo,
  elementExists,
  typeText,
  clickElement,
  wait,
  isBrowserRunning,
  closeBrowser,
} from "./browser.js";
import { getLatestResponseText, isGenerationComplete } from "./response-extractor.js";
import { fibonacciBackoff, sleep } from "./backoff.js";
import { getConversationId, setConversationId, clearConversation } from "./session-store.js";

export interface SessionState {
  isLoggedIn: boolean;
  currentModel: string | null;
  conversationId: string | null;
}

export interface AskResult {
  response: string;
  elapsed_seconds: number;
  model: string | null;
  chat_id: string | null;
  poll_count: number;
  error?: string;
}

let sessionState: SessionState = {
  isLoggedIn: false,
  currentModel: null,
  conversationId: null,
};

let sessionInitialized = false;

export async function ensureSession(): Promise<void> {
  if (sessionInitialized && isBrowserRunning()) {
    return;
  }

  // Load saved conversation from store
  const savedConversationId = await getConversationId();
  if (savedConversationId) {
    sessionState.conversationId = savedConversationId;
  }

  await launchBrowser();

  // Navigate to saved conversation or home page
  const targetUrl = sessionState.conversationId
    ? `https://chatgpt.com/c/${sessionState.conversationId}`
    : CONFIG.chatgptUrl;

  console.error(`[session] Navigating to: ${targetUrl}`);
  const success = await navigateTo(targetUrl);
  if (!success) {
    throw new Error("Failed to navigate to ChatGPT");
  }

  await wait(3000);

  // Check if already logged in
  let isLoggedIn = await checkLoginStatus();
  if (isLoggedIn) {
    sessionState.isLoggedIn = true;
    sessionInitialized = true;
    return;
  }

  // Not logged in - wait for user to login manually
  console.error("\n╔════════════════════════════════════════════════════════════╗");
  console.error("║  Please log in to ChatGPT in the browser window.         ║");
  console.error("║  The browser will wait for you to complete login.        ║");
  console.error("║  Press Ctrl+C to cancel.                                 ║");
  console.error("╚════════════════════════════════════════════════════════════╝\n");

  // Wait up to 5 minutes for user to login
  const maxWaitTime = 5 * 60 * 1000; // 5 minutes
  const checkInterval = 2000; // 2 seconds
  const startTime = Date.now();

  while (Date.now() - startTime < maxWaitTime) {
    await wait(checkInterval);
    isLoggedIn = await checkLoginStatus();
    if (isLoggedIn) {
      sessionState.isLoggedIn = true;
      sessionInitialized = true;
      console.error("\n✓ Login successful! Continuing...\n");
      return;
    }
  }

  throw new Error("Login timeout. Please try again.");
}

async function checkLoginStatus(): Promise<boolean> {
  const hasLoggedInIndicator = await elementExists(SELECTORS.loggedInIndicator);
  const hasLoginPrompt = await elementExists(SELECTORS.loginPrompt);
  const hasPromptArea = await elementExists(SELECTORS.promptTextarea);

  return (hasLoggedInIndicator || hasPromptArea) && !hasLoginPrompt;
}

async function navigateToConversation(): Promise<boolean> {
  // Check if we already have a conversation ID
  if (sessionState.conversationId) {
    const conversationUrl = `https://chatgpt.com/c/${sessionState.conversationId}`;
    console.error(`[session] Returning to conversation: ${conversationUrl}`);
    const success = await navigateTo(conversationUrl);
    if (success) {
      await wait(2000);
      // Verify we're on the conversation page
      const page = await getPage();
      const currentUrl = page.url();
      if (currentUrl.includes(`/c/${sessionState.conversationId}`)) {
        return true;
      }
    }
    // If navigation failed, clear the stored conversation
    console.error("[session] Stored conversation not found, will use current page");
    sessionState.conversationId = null;
  }

  // Check if current page is a conversation
  const page = await getPage();
  const currentUrl = page.url();
  const match = currentUrl.match(/\/c\/([a-f0-9-]+)/);
  if (match?.[1]) {
    sessionState.conversationId = match[1];
    await setConversationId(match[1], currentUrl);
    return true;
  }

  return false;
}

async function sendPromptText(prompt: string): Promise<void> {
  // Navigate to existing conversation or stay on current page
  await navigateToConversation();

  const typed = await typeText(SELECTORS.promptTextarea, prompt);
  if (!typed) {
    throw new Error("Failed to find prompt textarea. The ChatGPT UI may have changed.");
  }

  await wait(500);

  const sent = await clickElement(SELECTORS.sendButton);
  if (!sent) {
    const page = await getPage();
    await page.keyboard.press("Enter");
  }

  await wait(1000);

  // Extract and store conversation ID from URL
  const page = await getPage();
  const url = page.url();
  const match = url.match(/\/c\/([a-f0-9-]+)/);
  if (match?.[1]) {
    sessionState.conversationId = match[1];
    await setConversationId(match[1], url);
  }
}

async function pollUntilComplete(
  timeoutMinutes: number,
): Promise<{ response: string; pollCount: number; elapsedSeconds: number }> {
  const startTime = Date.now();
  const deadline = startTime + timeoutMinutes * 60 * 1000;
  let pollCount = 0;
  let lastContentLength = 0;
  let stableCount = 0;

  while (Date.now() < deadline) {
    const waitMs = fibonacciBackoff(pollCount);
    await sleep(waitMs);
    pollCount++;

    const result = await isGenerationComplete(lastContentLength, stableCount);
    lastContentLength = result.contentLength;
    stableCount = result.newStableCount;

    if (result.complete) {
      const response = await getLatestResponseText();
      const elapsedSeconds = Math.floor((Date.now() - startTime) / 1000);
      return {
        response: response ?? "",
        pollCount,
        elapsedSeconds,
      };
    }
  }

  const elapsedSeconds = Math.floor((Date.now() - startTime) / 1000);
  throw new Error(`Timeout after ${timeoutMinutes} minutes (${elapsedSeconds}s). Response may still be generating.`);
}

export async function blockingAsk(
  prompt: string,
  timeoutMinutes = 5,
): Promise<AskResult> {
  try {
    await ensureSession();
    await sendPromptText(prompt);
    const result = await pollUntilComplete(timeoutMinutes);

    return {
      response: result.response,
      elapsed_seconds: result.elapsedSeconds,
      model: sessionState.currentModel,
      chat_id: sessionState.conversationId,
      poll_count: result.pollCount,
    };
  } catch (error) {
    return {
      response: "",
      elapsed_seconds: 0,
      model: null,
      chat_id: null,
      poll_count: 0,
      error: error instanceof Error ? error.message : String(error),
    };
  }
}

export async function blockingReply(
  prompt: string,
  timeoutMinutes = 5,
): Promise<AskResult> {
  try {
    await ensureSession();
    await sendPromptText(prompt);
    const result = await pollUntilComplete(timeoutMinutes);

    return {
      response: result.response,
      elapsed_seconds: result.elapsedSeconds,
      model: sessionState.currentModel,
      chat_id: sessionState.conversationId,
      poll_count: result.pollCount,
    };
  } catch (error) {
    return {
      response: "",
      elapsed_seconds: 0,
      model: null,
      chat_id: null,
      poll_count: 0,
      error: error instanceof Error ? error.message : String(error),
    };
  }
}

export async function newConversation(): Promise<{ success: boolean; message: string }> {
  try {
    await ensureSession();
    // Clear the stored conversation
    await clearConversation();
    sessionState.conversationId = null;
    // Navigate to home to start fresh
    await navigateTo(CONFIG.chatgptUrl);
    await wait(1500);
    return { success: true, message: "New conversation started." };
  } catch (error) {
    return {
      success: false,
      message: `Failed to start new conversation: ${error instanceof Error ? error.message : String(error)}`,
    };
  }
}

export async function shutdownBrowser(): Promise<void> {
  await closeBrowser();
  sessionInitialized = false;
  sessionState = { isLoggedIn: false, currentModel: null, conversationId: null };
}

