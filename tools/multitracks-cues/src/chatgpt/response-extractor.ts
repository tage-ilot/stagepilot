import { getPage } from "./browser.js";

const PHRASES_TO_REMOVE = [
  "ChatGPT said:",
  "ChatGPT said",
  "Pro thinking",
  "Answer now",
  "Extended thinking",
  "Show thinking",
  "Hide thinking",
  "Reasoning",
  "Thinking...",
  "Thinking\u2026",
  "\u2022 ",
];

function cleanText(text: string): string {
  let cleaned = text;
  for (const phrase of PHRASES_TO_REMOVE) {
    while (cleaned.includes(phrase)) {
      cleaned = cleaned.replace(phrase, "");
    }
  }
  cleaned = cleaned.replace(/^Thinking\s*/i, "");
  cleaned = cleaned.replace(/Pro\s+thinking\s*\u2022?\s*/gi, "");
  cleaned = cleaned.replace(/^\d+\s*(seconds?|secs?)\s*/i, "");
  cleaned = cleaned.replace(/\s+/g, " ").trim();
  return cleaned;
}

export async function getLatestResponseText(): Promise<string | null> {
  try {
    const page = await getPage();

    // Strategy 1: conversation turns
    const turnsText = await page.evaluate(() => {
      const turns = document.querySelectorAll('[data-testid^="conversation-turn-"]');
      if (turns.length >= 2) {
        const lastTurn = turns[turns.length - 1];
        return (lastTurn as HTMLElement).innerText?.trim() || null;
      }
      return null;
    });

    if (turnsText) {
      const cleaned = cleanText(turnsText);
      if (cleaned.length > 0) return cleaned;
    }

    // Strategy 2: assistant role
    const assistantText = await page.evaluate(() => {
      const assistantMsgs = document.querySelectorAll('[data-message-author-role="assistant"]');
      if (assistantMsgs.length > 0) {
        const lastMsg = assistantMsgs[assistantMsgs.length - 1];
        return (lastMsg as HTMLElement).innerText?.trim() || null;
      }
      return null;
    });

    if (assistantText) {
      return cleanText(assistantText);
    }

    // Strategy 3: markdown/prose containers
    const markdownText = await page.evaluate(() => {
      const markdown = document.querySelector('.markdown, .prose, [class*="markdown"]');
      return markdown?.textContent?.trim() || null;
    });

    if (markdownText) {
      return cleanText(markdownText);
    }

    return null;
  } catch (error) {
    console.error("Failed to get response text:", error);
    return null;
  }
}

export async function isGenerationComplete(
  lastContentLength: number,
  stableCount: number,
): Promise<{ complete: boolean; contentLength: number; newStableCount: number }> {
  const page = await getPage();

  const indicators = await page.evaluate(() => {
    const turns = document.querySelectorAll('[data-testid^="conversation-turn-"]');
    let lastTurnHasCopy = false;
    let isThinking = false;

    if (turns.length >= 2) {
      const lastTurn = turns[turns.length - 1]!;
      lastTurnHasCopy = !!lastTurn.querySelector('[data-testid="copy-turn-action-button"]');
      const turnText = (lastTurn as HTMLElement).innerText || "";
      const hasThinkingUI = !!lastTurn.querySelector(
        '[class*="thinking"], [class*="reasoning"], [data-testid*="thinking"]',
      );
      isThinking = hasThinkingUI || (/\b(thinking|reasoning)\b/i.test(turnText) && turnText.length < 200);
    }

    return { turnCount: turns.length, lastTurnHasCopy, isThinking };
  });

  const currentText = await getLatestResponseText();
  const currentLength = currentText?.length ?? 0;

  let newStableCount = stableCount;
  if (currentLength > 0 && currentLength === lastContentLength) {
    newStableCount++;
  } else {
    newStableCount = 0;
  }

  const FALLBACK_STABLE_THRESHOLD = 10;
  const highConfidence =
    (indicators.lastTurnHasCopy && currentLength > 0 && newStableCount >= 1) ||
    (!indicators.isThinking && currentLength > 0 && newStableCount >= FALLBACK_STABLE_THRESHOLD);

  return {
    complete: highConfidence,
    contentLength: currentLength,
    newStableCount,
  };
}

