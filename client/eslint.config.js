import js from '@eslint/js';
import globals from 'globals';
import tseslint from 'typescript-eslint';

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
  {
    // Rendering is behind the render interface (ADR 0002): only src/render/three may import three.
    files: ['src/**/*.ts'],
    ignores: ['src/render/three/**'],
    rules: {
      'no-restricted-imports': [
        'error',
        {
          patterns: [{ regex: '^three(/|$)', message: 'Use the render interface (src/render).' }],
        },
      ],
    },
  },
  {
    files: ['eslint.config.js'],
    ...tseslint.configs.disableTypeChecked,
  },
);
