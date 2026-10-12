export const OFFLINE_SENTENCE = "Can't reach StagePilot. Check that it is running, then try again.";
const SERVER_SENTENCE = "StagePilot hit an error. Try again.";
const REFUSED_SENTENCE = "StagePilot could not do that. Check the values and try again.";

// A server `detail` is only shown when it reads like a sentence an operator can act on.
const readable = (text: string) =>
  /\s/.test(text) && !/[{}[\]<>]|undefined|null|Traceback|\bat\s.+:\d+/.test(text) && text.length <= 200;

/**
 * Turns anything a failed request can throw into one plain sentence for an inline alert.
 * Never returns raw transport text ("Failed to fetch"), raw status lines, enums or "undefined".
 */
export function describeError(cause: unknown, fallback: string): string {
  // Duck-typed on `status` so this stays independent of the api module (ApiError carries a numeric status).
  if (cause instanceof Error && typeof (cause as { status?: unknown }).status === "number") {
    const status = (cause as Error & { status: number }).status;
    if (status === 401 || status === 403) return cause.message;
    if (status >= 500) return SERVER_SENTENCE;
    if (status === 429) return "Too many requests. Wait a moment, then try again.";
    return readable(cause.message) && !/^StagePilot API returned/.test(cause.message)
      ? cause.message : REFUSED_SENTENCE;
  }
  // fetch rejects with TypeError (Chrome "Failed to fetch", Safari "Load failed"), timeouts with AbortError.
  if (cause instanceof TypeError || ["AbortError", "TimeoutError"].includes((cause as { name?: string } | null)?.name ?? "")) {
    return OFFLINE_SENTENCE;
  }
  if (cause instanceof Error && readable(cause.message)) return cause.message;
  return fallback;
}
