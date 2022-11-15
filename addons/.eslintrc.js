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
      workspaceRelative('reviewstack/tsconfig.json'),
      workspaceRelative('reviewstack.dev/tsconfig.json'),
      workspaceRelative('shared/tsconfig.json'),
      workspaceRelative('textmate/tsconfig.json'),
      workspaceRelative('vscode/tsconfig.json'),
    ],
    sourceType: 'module',
  },
  plugins: [
    // Brings the typescript parser used above
    '@typescript-eslint',
    // enforce rules of hooks and hook dependencies
    'react-hooks',
    // Sorting imports is hard...
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
    'reviewstack/src/generated/**',
    'reviewstack/codegen.js',
    'reviewstack/textmate.js',
    'reviewstack.dev/build.js',
    'reviewstack.dev/release.js',
    'reviewstack.dev/start.js',
    'node_modules/**',
  ],
  rules: {
    // Need to use the TypeScript version of no-unused-vars so it understands
    // "private" constructor args.
    '@typescript-eslint/no-unused-vars': ['error', {argsIgnorePattern: '^_'}],
    '@typescript-eslint/consistent-type-imports': 'error',

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
    'no-return-await': 'error',
    'object-shorthand': 'error',
    'prefer-arrow-callback': 'error',
    'react-hooks/rules-of-hooks': 'error',
    // https://recoiljs.org/docs/introduction/installation/#eslint
    'react-hooks/exhaustive-deps': [
      'error',
      {additionalHooks: '(useRecoilCallback|useRecoilTransaction_UNSTABLE)'},
    ],
    'sort-imports': 'off',
    yoda: 'error',

    // Custom rules
    'rulesdir/recoil-key-matches-variable': 'error',

    // WARNINGS
    'require-await': 'warn',
    'no-async-promise-executor': 'warn',
  },
};
