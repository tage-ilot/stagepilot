import { execSync } from "node:child_process";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "tailwindcss";
import autoprefixer from "autoprefixer";
import theme from "./tailwind.config.js";

// What this build shows, for the "Last built from" line at the top of the page (never a guess: "unknown" when git is not there).
const git = (args: string) => { try { return execSync(`git ${args}`, { stdio: ["ignore", "pipe", "ignore"] }).toString().trim(); } catch { return ""; } };
const build = { commit: git("rev-parse --short=8 HEAD") || "unknown", builtAt: new Date().toISOString().slice(0, 16).replace("T", " ") + " UTC", dirty: git("status --porcelain --untracked-files=no -- ..").length > 0 };

// The section gallery (a review tool, never shipped): its own entry, its own output folder, no editor plugin. The app's own build
// (vite.config.ts) names only index.html, so sections.html and src/gallery/ are not in the production or the desktop bundle.
export default defineConfig({
  base: "./",
  define: { __GALLERY_BUILD__: JSON.stringify(build) },
  plugins: [react()],
  server: { host: "127.0.0.1", port: 5175, strictPort: true },
  css: {
    postcss: {
      plugins: [tailwindcss({
        ...theme,
        content: ["./sections.html", "./src/components/**/*.{ts,tsx}", "./src/gallery/**/*.{ts,tsx}", "!./src/**/*.test.*"],
      }), autoprefixer()],
    },
  },
  // The UI gallery (`npm run ui:gallery:build`) writes to the same folder: neither build empties it.
  build: { outDir: "dist-gallery", emptyOutDir: false, rollupOptions: { input: "sections.html" } },
});
