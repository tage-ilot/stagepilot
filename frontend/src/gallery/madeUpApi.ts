/**
 * The gallery has no backend. The only requests its blocks can make are the helper setup boxes asking for their one-time command
 * (and ending it): they are answered here with a made-up command that goes nowhere. Anything else is a plain 404.
 */
export function installMadeUpApi(): void {
  const real = window.fetch.bind(window);
  window.fetch = async (input, init) => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    const path = new URL(url, window.location.href).pathname;
    const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
    if (path.startsWith("/api/") && path.endsWith("/helper-install")) {
      if ((init?.method ?? "GET") === "DELETE") return json({ ended: true });
      return json({
        command: "curl -fsS http://192.0.2.20:8000/helper/made-up/install.sh | sh", url: "http://192.0.2.20:8000/helper/made-up/install.sh",
        expires_at: new Date(Date.now() + 30 * 60_000).toISOString(), lan_ip: "192.0.2.20", port: 8000, bundled_version: "0.0.0",
      });
    }
    if (path.startsWith("/api/")) return json({ error: "The section gallery has no backend." }, 404);
    return real(input, init);
  };
}
