import { chromium, type BrowserContext, type Page } from "playwright";
import { mkdir } from "node:fs/promises";
import { existsSync } from "node:fs";
import os from "node:os";
import path from "node:path";

export const CONFIG = {
  chatgptUrl: "https://chatgpt.com",
  userDataDir: path.join(os.homedir(), ".stagepilot", "chatgpt-mcp", "user-data"),
  defaultTimeout: 30_000,
  typingDelay: 50,
} as const;

export const SELECTORS = {
  promptTextarea: [
    "#prompt-textarea",
    '[data-testid="prompt-textarea"]',
    'textarea[placeholder*="Message"]',
    'div[contenteditable="true"]',
  ],
  sendButton: [
    '[data-testid="send-button"]',
    'button[aria-label*="Send"]',
    'button[data-testid="composer-send-button"]',
  ],
  loggedInIndicator: [
    '[data-testid="profile-button"]',
    'button[aria-label*="Profile"]',
    'img[alt*="User"]',
  ],
  loginPrompt: [
    'button:has-text("Log in")',
    'a:has-text("Log in")',
    '[data-testid="login-button"]',
  ],
} as const;

let context: BrowserContext | null = null;
let page: Page | null = null;

async function ensureUserDataDir(): Promise<void> {
  if (!existsSync(CONFIG.userDataDir)) {
    await mkdir(CONFIG.userDataDir, { recursive: true });
  }
}

export async function launchBrowser(): Promise<Page> {
  if (page && !page.isClosed()) {
    return page;
  }

  await ensureUserDataDir();

  context = await chromium.launchPersistentContext(CONFIG.userDataDir, {
    headless: false,
    args: ["--disable-blink-features=AutomationControlled", "--no-sandbox"],
    userAgent:
      "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
    viewport: { width: 1280, height: 800 },
  });

  const pages = context.pages();
  page = pages.length > 0 ? pages[0]! : await context.newPage();

  return page;
}

export async function getPage(): Promise<Page> {
  if (!page || page.isClosed()) {
    return launchBrowser();
  }
  return page;
}

export function isBrowserRunning(): boolean {
  return page !== null && !page.isClosed();
}

export async function closeBrowser(): Promise<void> {
  if (context) {
    await context.close();
  }
  page = null;
  context = null;
}

export async function navigateTo(url: string): Promise<boolean> {
  const p = await getPage();
  try {
    await p.goto(url, { waitUntil: "domcontentloaded", timeout: CONFIG.defaultTimeout });
    return true;
  } catch (error) {
    console.error(`Failed to navigate to ${url}:`, error);
    return false;
  }
}

export async function findElement(selectors: readonly string[], timeout = 5000): Promise<ReturnType<Page["waitForSelector"]> | null> {
  const p = await getPage();
  for (const selector of selectors) {
    try {
      const element = await p.waitForSelector(selector, { timeout, state: "visible" });
      if (element) return element;
    } catch {
      // try next selector
    }
  }
  return null;
}

export async function elementExists(selectors: readonly string[]): Promise<boolean> {
  const p = await getPage();
  for (const selector of selectors) {
    try {
      const element = await p.$(selector);
      if (element) return true;
    } catch {
      // try next selector
    }
  }
  return false;
}

export async function typeText(selectors: readonly string[], text: string): Promise<boolean> {
  const element = await findElement(selectors);
  if (!element) return false;
  await element.click();
  await element.fill("");
  for (const char of text) {
    await element.type(char, { delay: CONFIG.typingDelay });
  }
  return true;
}

export async function clickElement(selectors: readonly string[]): Promise<boolean> {
  const element = await findElement(selectors);
  if (!element) return false;
  await element.click();
  return true;
}

export async function wait(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

