import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";

export default tseslint.config(
  {
    ignores: [
      "dist/**",
      "node_modules/**",
      ".pnpm-store/**",
      ".tools/**",
      ".superpowers/**",
      ".worktrees/**",
      "src-tauri/**",
      "src-tauri/target/**",
      "components.json",
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["scripts/**/*.{js,mjs}", "docs/research/probes/**/*.{js,mjs}"],
    languageOptions: { globals: globals.node },
  },
  {
    files: [
      "src/**/*.{ts,tsx,js,mjs}",
      "tests/**/*.{ts,tsx,js,mjs}",
      "e2e/**/*.{ts,tsx,js,mjs}",
      "*.config.{ts,js,mjs}",
      "eslint.config.js",
    ],
    languageOptions: {
      globals: { ...globals.browser, ...globals.node },
    },
    plugins: {
      "react-hooks": reactHooks,
    },
    rules: {
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "warn",
    },
  },
);
