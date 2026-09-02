import js from "@eslint/js";
import globals from "globals";
import react from "eslint-plugin-react";
import reactHooks from "eslint-plugin-react-hooks";

export default [
  { ignores: ["dist"] },
  js.configs.recommended,
  reactHooks.configs.flat["recommended-latest"],
  {
    files: ["**/*.{js,jsx}"],
    // Without this plugin, no-unused-vars does not see a name used in JSX.
    plugins: { react },
    languageOptions: {
      ecmaVersion: "latest",
      globals: globals.browser,
      parserOptions: {
        ecmaFeatures: { jsx: true },
        sourceType: "module",
      },
    },
    rules: {
      "react/jsx-uses-react": "error",
      "react/jsx-uses-vars": "error",
    },
  },
  {
    // Config files run in node, not the browser.
    files: ["*.config.js"],
    languageOptions: { globals: globals.node },
  },
];
