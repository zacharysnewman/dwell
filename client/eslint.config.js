import js from '@eslint/js';
import globals from 'globals';
import tseslint from 'typescript-eslint';

const NO_THREE = { regex: '^three(/|$)', message: 'Use the render interface (src/render).' };
const NO_NODE = { regex: '^node:', message: 'Node built-ins are only allowed in tests.' };

function restrictImports(files, ignores, patterns) {
  return [{ files, ignores, rules: { 'no-restricted-imports': ['error', { patterns }] } }];
}

export default tseslint.config(
  { ignores: ['dist/'] },
  js.configs.recommended,
  ...tseslint.configs.strictTypeChecked,
  {
    languageOptions: {
      globals: { ...globals.browser },
      parserOptions: {
        project: ['./tsconfig.json', './tsconfig.node.json'],
        tsconfigRootDir: import.meta.dirname,
      },
    },
  },
  // Import boundaries. Rendering is behind the render interface (ADR 0002): only src/render/three
  // may import three. Node built-ins are for tests only (the client runs in browsers).
  ...restrictImports(
    ['src/**/*.ts'],
    ['src/render/three/**', 'src/**/*.test.ts'],
    [NO_THREE, NO_NODE],
  ),
  ...restrictImports(['src/render/three/**/*.ts'], ['src/**/*.test.ts'], [NO_NODE]),
  ...restrictImports(['src/**/*.test.ts'], ['src/render/three/**'], [NO_THREE]),
  {
    files: ['eslint.config.js'],
    ...tseslint.configs.disableTypeChecked,
  },
);
