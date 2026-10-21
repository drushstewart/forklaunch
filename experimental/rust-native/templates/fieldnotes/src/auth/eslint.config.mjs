import tseslint from "typescript-eslint";
export default [
  { ignores: ["dist/**", "node_modules/**", "tests/**"] },
  ...tseslint.configs.recommended,
  {
    rules: {
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_" },
      ],
    },
  },
];
