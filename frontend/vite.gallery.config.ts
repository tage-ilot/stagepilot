import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "tailwindcss";
import autoprefixer from "autoprefixer";
import theme from "./tailwind.config.js";

// The section gallery (a review tool, never shipped): its own entry, its own output folder, no editor plugin. The app's own build
// (vite.config.ts) names only index.html, so sections.html and src/gallery/ are not in the production or the desktop bundle.
export default defineConfig({
  base: "./",
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
