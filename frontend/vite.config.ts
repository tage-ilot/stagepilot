import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import { visualEditor } from "./dev-editor/plugin";

export default defineConfig({
  // visualEditor is apply:"serve": absent from `vite build`. Host stays loopback unless `npm run dev:lan` passes --host.
  // The editor must run before react() so data-src line:col refer to the original source.
  plugins: [visualEditor({ frontendRoot: fileURLToPath(new URL(".", import.meta.url)) }), react()],
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
  },
});
