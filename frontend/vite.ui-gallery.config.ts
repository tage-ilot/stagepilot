import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "tailwindcss";
import autoprefixer from "autoprefixer";
import theme from "./tailwind.config.js";

export default defineConfig({
  base: "./",
  plugins: [react()],
  server: { host: "127.0.0.1", port: 5174, strictPort: true },
  css: {
    postcss: {
      plugins: [tailwindcss({
        ...theme,
        content: ["./ui-gallery.html", "./src/components/ui/**/*.{ts,tsx}", "!./src/components/ui/**/*.test.*"],
      }), autoprefixer()],
    },
  },
  build: { outDir: "dist-gallery", rollupOptions: { input: "ui-gallery.html" } },
});
