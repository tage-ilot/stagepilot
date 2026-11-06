import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    // The macOS Intel CI runner is several times slower than a laptop: Lighting tests that take 1 s here took 5 s there and
    // failed the release validation twice on the 5 s default. Real hangs still fail, just later.
    testTimeout: 20000,
    hookTimeout: 20000,
  },
});
