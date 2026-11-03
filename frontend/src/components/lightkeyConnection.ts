/** The live-link API port the backend assumes when the saved address names none. */
export const DEFAULT_API_PORT = 8770;

const norm = (host: string) => host.trim().toLowerCase();

/** Host and port of a saved or effective API address (`http://h:p`, `h:p`, or just `h`). */
export function splitApiUrl(url: string): { host: string; port: string } {
  const rest = (url.includes("://") ? url.slice(url.indexOf("://") + 3) : url).trim().replace(/\/+$/, "");
  if (!rest) return { host: "", port: String(DEFAULT_API_PORT) };
  const bracket = /^(\[[^\]]*\])(?::(\d+))?$/.exec(rest);
  if (bracket) return { host: bracket[1]!, port: bracket[2] ?? String(DEFAULT_API_PORT) };
  const colon = rest.lastIndexOf(":");
  if (colon > 0 && rest.indexOf(":") === colon && /^\d+$/.test(rest.slice(colon + 1))) {
    return { host: rest.slice(0, colon), port: rest.slice(colon + 1) };
  }
  return { host: rest, port: String(DEFAULT_API_PORT) };
}

export const apiPortError = (port: string): string | null => {
  const value = Number(port);
  return port.trim() !== "" && Number.isInteger(value) && value >= 1 && value <= 65535 ? null : "Enter an API port from 1 to 65535.";
};

/**
 * What the UI writes to `lightkey.api_url`: empty when the API is on the default port of the saved OSC host (the backend then
 * derives it, as it always did), otherwise `<host>:<port>`.
 */
export function composeApiUrl({ host, apiPort, savedOscHost }: { host: string; apiPort: string; savedOscHost: string }): string {
  const clean = host.trim();
  if (apiPort.trim() === String(DEFAULT_API_PORT) && norm(clean) === norm(savedOscHost)) return "";
  return `${clean}:${apiPort.trim()}`;
}

/** The host of a saved API address when it is NOT the OSC host (null when they agree or nothing is saved). */
export function apiHostMismatch(effectiveApiUrl: string, savedOscHost: string): string | null {
  const { host } = splitApiUrl(effectiveApiUrl);
  if (!host || norm(host) === norm(savedOscHost)) return null;
  return host;
}
