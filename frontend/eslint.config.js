import js from "@eslint/js";
import globals from "globals";
import react from "eslint-plugin-react";
import reactHooks from "eslint-plugin-react-hooks";

const restrictImports = (files, patterns) => ({
  files,
  rules: { "no-restricted-imports": ["error", { patterns }] },
});

const FEATURE_NO_PAGES_OR_APP = {
  regex: "^(\\.\\./)+(pages|app)(/|$)",
  message: "features/ never imports pages/ or app/.",
};

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
  // Layering, mirroring the backend's `core/` + `features/` split:
  //   core/     imports only core/
  //   features/ imports core/, and another feature only via its index.js
  //   pages/    imports core/ and feature index.js files
  //   app/      the only importer of pages/
  // Every import is relative, so each rule matches on the relative path as
  // written from that folder's depth.
  restrictImports(
    ["src/core/**/*.{js,jsx}"],
    [
      {
        regex: "^(\\.\\./)+(features|pages|app)(/|$)",
        message: "core/ imports only core/.",
      },
    ],
  ),
  restrictImports(
    ["src/features/*/*.{js,jsx}"],
    [
      FEATURE_NO_PAGES_OR_APP,
      {
        regex: "^\\.\\./[^./][^/]*/",
        message:
          "Import another feature through its index.js (`../other`), not a file inside it.",
      },
    ],
  ),
  restrictImports(
    ["src/features/*/*/*.{js,jsx}"],
    [
      FEATURE_NO_PAGES_OR_APP,
      {
        regex: "^\\.\\./\\.\\./[^./][^/]*/",
        message:
          "Import another feature through its index.js (`../../other`), not a file inside it.",
      },
    ],
  ),
  restrictImports(
    ["src/pages/**/*.{js,jsx}"],
    [
      {
        regex: "^\\.\\./features/[^/]+/",
        message:
          "pages/ imports a feature through its index.js (`../features/x`).",
      },
      {
        regex: "^(\\./|\\.\\./app(/|$))",
        message: "pages/ imports only core/ and feature index.js files.",
      },
    ],
  ),
  {
    // Config files run in node, not the browser.
    files: ["*.config.js"],
    languageOptions: { globals: globals.node },
  },
];
