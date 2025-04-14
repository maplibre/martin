import eslint from "@eslint/js";
import reactPlugin from "eslint-plugin-react";
import reactRefresh from "eslint-plugin-react-refresh";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  eslint.configs.recommended,
  {
    files: ["src/**/*.{ts,tsx}"],
    ...reactPlugin.configs.flat.recommended,
    languageOptions: {
      ...reactPlugin.configs.flat.recommended.languageOptions,
      globals: {
        ...globals.browser,
      },
    },
  },
  {
    files: ["src/**/*.{ts,tsx}"],
    ...reactPlugin.configs.flat["jsx-runtime"],
    languageOptions: {
      ...reactPlugin.configs.flat["jsx-runtime"].languageOptions,
      globals: {
        ...globals.browser,
      },
    },
  },
  reactRefresh.configs.vite,
  tseslint.configs.recommended,
);
