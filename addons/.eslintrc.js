/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

const path = require('path');
const workspaceRelative = p => path.resolve(path.join(__dirname, p));

// Set the local directory that contains custom rules
const rulesDirPlugin = require('eslint-plugin-rulesdir');
rulesDirPlugin.RULES_DIR = workspaceRelative('eslint-rules');

module.exports = {
  root: true,
  parser: '@typescript-eslint/parser',
  parserOptions: {
    ecmaVersion: 'latest',
    project: [
      workspaceRelative('isl/tsconfig.json'),
      workspaceRelative('isl-server/tsconfig.json'),
      workspaceRelative('screenshot-tool/tsconfig.json'),
      workspaceRelative('shared/tsconfig.json'),
      workspaceRelative('components/tsconfig.json'),
      workspaceRelative('textmate/tsconfig.json'),
      workspaceRelative('vscode/tsconfig.json'),
      workspaceRelative('scripts/tsconfig.json'),
    ],
    sourceType: 'module',
  },
  plugins: [
    // Brings the typescript parser used above
    '@typescript-eslint',
    // enforce rules of hooks and hook dependencies
    'react-hooks',
    // Sorting imports is maintained by prettier-plugin-organize-imports
    'import',
    // Allow locally defined custom rules
    'rulesdir',
  ],
  extends: [
    'eslint:recommended',
    'plugin:@typescript-eslint/eslint-recommended',
    'plugin:@typescript-eslint/recommended',
  ],
  ignorePatterns: [
    '.eslintrc.js',
    'isl/build.js',
    'isl/release.js',
    'isl/start.js',
    'isl-server/codegen.js',
    // @fb-only
    // @fb-only
    'node_modules/**',
  ],
  rules: {
    // Need to use the TypeScript version of no-unused-vars so it understands
    // "private" constructor args.
    '@typescript-eslint/no-unused-vars': ['warn', {argsIgnorePattern: '^_'}],
    '@typescript-eslint/consistent-type-imports': 'error',

    curly: 'error',
    'dot-notation': 'error',
    'import/no-duplicates': 'error',
    // Sorting imports is maintained by prettier-plugin-organize-imports
    'import/order': 'off',
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
    'no-return-await': 'error',
    'object-shorthand': 'error',
    'prefer-arrow-callback': 'error',
    'react-hooks/rules-of-hooks': 'error',
    'react-hooks/exhaustive-deps': 'error',
    'sort-imports': 'off',
    yoda: 'error',

    'no-restricted-imports': [
      'error',
      {
        paths: [
          {
            name: 'jotai/utils',
            importNames: ['atomFamily'],
            message:
              'atomFamily leaks memory. Use atomFamilyWeak(keyToAtom), or cached(keyToAtom), or useAtomValue(useMemo(() => keyToAtom(k), [k])), or useAtomGet and useAtomHas instead.',
          },
          {
            name: 'react-dom/test-utils',
            importNames: ['act'],
            message: 'Prefer importing act from @testing-library/react instead.',
          },
        ],
      },
    ],

    // Custom rules
    'rulesdir/jotai-maybe-use-family': 'error',
    'rulesdir/stylex-import': 'error',
    'rulesdir/internal-promise-callback-types': 'error',
    'rulesdir/no-facebook-imports': 'error',

    // WARNINGS
    'require-await': 'warn',
    'no-async-promise-executor': 'warn',
  },
};
