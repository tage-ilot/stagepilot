const SENSITIVE_KEY = /(?:access_token|refresh_token|id_token|token|authorization|code|verifier|secret|cookie|login.?id|account.?id|auth.?url)/i;
const BEARER = /\bBearer\s+[A-Za-z0-9._~+/-]+=*/gi;
const URL_QUERY = /(https?:\/\/[^\s?]+)\?[^\s"'<>]+/gi;
const EMAIL = /\b([A-Z0-9._%+-])[A-Z0-9._%+-]*@([A-Z0-9.-]+\.[A-Z]{2,})\b/gi;
const DEVICE_CODE = /\b[A-Z0-9]{4}-[A-Z0-9]{4}\b/g;
const USER_HOME = /(?:[A-Z]:\\Users\\|\/Users\/)[^/\\\s]+/gi;

export function redactString(value: string): string {
  return value
    .replace(BEARER, "Bearer [REDACTED]")
    .replace(URL_QUERY, "$1?[REDACTED]")
    .replace(EMAIL, "$1***@$2")
    .replace(DEVICE_CODE, "[REDACTED-CODE]")
    .replace(USER_HOME, "[USER_HOME]");
}

export function redact(value: unknown): unknown {
  if (typeof value === "string") return redactString(value);
  if (Array.isArray(value)) return value.map(redact);
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [
        key,
        SENSITIVE_KEY.test(key) ? "[REDACTED]" : redact(item),
      ]),
    );
  }
  return value;
}

export function sanitizedError(error: unknown): string {
  return redactString(error instanceof Error ? error.message : String(error));
}

