import eslint from "@eslint/js";
import globals from "globals";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import tseslint from "typescript-eslint";
import { sectionGuard } from "./eslint.sections.js";

export default tseslint.config(
  { ignores: ["dist", "scripts/**"] }, // scripts/: manual measurement tools run with node + Playwright, not part of the app
  eslint.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
    },
    plugins: {
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "react-refresh/only-export-components": ["warn", { allowConstantExport: true }],
    },
  },
  // A configuration page is built from the section templates only (DESIGN.md 6.6): see eslint.sections.js for the rules and their messages.
  sectionGuard,
);
