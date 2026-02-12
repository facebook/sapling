/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

const eslint = require('@eslint/js');
const tseslint = require('typescript-eslint');
const reactHooksPlugin = require('eslint-plugin-react-hooks');
const importPlugin = require('eslint-plugin-import');
const globals = require('globals');

module.exports = tseslint.config(
  eslint.configs.recommended,
  ...tseslint.configs.recommended,
  {
    ignores: [
      'eslint.config.js',
      'reviewstack/src/generated/**',
      'reviewstack/codegen.js',
      'reviewstack/textmate.js',
      'reviewstack.dev/build.js',
      'reviewstack.dev/release.js',
      'reviewstack.dev/start.js',
      'vscode/facebook/buildInternalExtension.*',
      '**/node_modules/**',
      '**/dist/**',
      '**/build/**',
    ],
  },
  {
    files: ['**/*.ts', '**/*.tsx'],
    languageOptions: {
      ecmaVersion: 'latest',
      sourceType: 'module',
      globals: {
        ...globals.browser,
        ...globals.node,
      },
      parserOptions: {
        projectService: true,
        tsconfigRootDir: __dirname,
      },
    },
    plugins: {
      'react-hooks': reactHooksPlugin,
      import: importPlugin,
    },
    rules: {
      // TypeScript rules
      '@typescript-eslint/no-unused-vars': ['warn', {argsIgnorePattern: '^_'}],
      '@typescript-eslint/consistent-type-imports': 'error',
      '@typescript-eslint/no-require-imports': 'off',

      // General rules
      curly: 'error',
      'dot-notation': 'error',
      'import/no-duplicates': 'error',
      'import/order': [
        'error',
        {
          groups: ['type'],
          'newlines-between': 'always',
          alphabetize: {
            order: 'asc',
            caseInsensitive: false,
          },
        },
      ],
      'no-await-in-loop': 'error',
      'no-bitwise': 'error',
      'no-caller': 'error',
      'no-console': 'warn',
      'no-constant-condition': ['error', {checkLoops: false}],
      'no-debugger': 'error',
      'no-duplicate-case': 'error',
      'no-empty': ['error', {allowEmptyCatch: true}],
      'no-eval': 'error',
      'no-ex-assign': 'error',
      'no-fallthrough': ['error', {commentPattern: '.*'}],
      'no-new-func': 'error',
      'no-new-wrappers': 'error',
      'no-param-reassign': 'error',
      'no-return-await': 'error',
      'no-script-url': 'error',
      'no-self-compare': 'error',
      'no-unsafe-finally': 'error',
      'no-unused-expressions': ['error', {allowShortCircuit: true, allowTernary: true}],
      'no-var': 'error',
      'object-shorthand': 'error',
      'prefer-arrow-callback': 'error',
      'sort-imports': 'off',
      yoda: 'error',

      // React Hooks rules
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': [
        'error',
        {additionalHooks: '(useRecoilCallback|useRecoilTransaction_UNSTABLE)'},
      ],

      // Restricted imports
      'no-restricted-imports': [
        'error',
        {
          paths: [
            {
              name: 'jotai/utils',
              importNames: ['atomFamily'],
              message:
                'atomFamily leaks memory. Use atomFamilyWeak(keyToAtom), or cached(keyToAtom), or useAtomValue(useMemo(() => keyToAtom(k), [k])) instead.',
            },
          ],
        },
      ],

      // Warnings
      'require-await': 'warn',
      'no-async-promise-executor': 'warn',
    },
  },
);
