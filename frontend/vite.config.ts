import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import { visualEditor } from "./dev-editor/plugin";

// Optional: STAGEPILOT_DEV_API_PROXY=http://127.0.0.1:8765 forwards /api and /ws to a running backend,
// so the editor can drive every screen from a LAN browser (dev server only; unset by default).
const apiProxy = process.env.STAGEPILOT_DEV_API_PROXY;

// Optional: STAGEPILOT_DEV_ALLOWED_HOSTS=sp.example.com,other.example.com lets the dev server answer to those host names when it is
// reached through a DNS name or reverse proxy (Vite otherwise refuses unknown Host headers). Dev server only; unset by default.
const extraHosts = (process.env.STAGEPILOT_DEV_ALLOWED_HOSTS ?? "").split(",").map((host) => host.trim()).filter(Boolean);

export default defineConfig({
  // visualEditor is apply:"serve": absent from `vite build`. Host stays loopback unless `npm run dev:lan` passes --host.
  // The editor must run before react() so data-src line:col refer to the original source.
  plugins: [visualEditor({ frontendRoot: fileURLToPath(new URL(".", import.meta.url)) }), react()],
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
    ...(extraHosts.length ? { allowedHosts: extraHosts } : {}),
    ...(apiProxy ? { proxy: { "/api": apiProxy, "/ws": { target: apiProxy, ws: true } } } : {}),
  },
});
