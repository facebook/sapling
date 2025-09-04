/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

const path = require('path');

// Note: there are 2 versions of prettiers:
// - Local prettier. Affects `yarn run prettier`, and editors like `nvim`.
// - Monorepo prettier. Affects `arc lint`, and the internal VSCode.
// Thoroughly test when making changes.

const config = {
  arrowParens: 'avoid',
  bracketSpacing: false,
  bracketSameLine: true,
  useTabs: false,
  singleQuote: true,
  tabWidth: 2,
  printWidth: 100,
  trailingComma: 'all',
  overrides: [
    {
      files: ['**/*.{ts,tsx}'],
      options: {
        parser: 'typescript',
      },
    },
  ],
};

// `arc lint` runs the monorepo prettier, with cwd == monorepo root
// related code path: tools/arcanist/lint/external/prettier_linter.js
const isArcLint = process.env.ARC2_COMMAND != null; // could be: 'lint', 'linttool', 'f'
if (isArcLint) {
  // Use prettier2's "plugin search" to discover and load the plugin.
  // (see "externalAutoLoadPluginInfos" in tools/third-party/prettier/node_modules/prettier/index.js)
  // Need a different approach for prettier3 (https://github.com/prettier/prettier/pull/14759).
  config.pluginSearchDirs = ['.'];
} else {
  // Explicitly set the plugin.
  // Does not work with the monorepo prettier (arc lint).
  // - `prettier-plugin-organize-imports` cannot be imported from monorepo root.
  // - `require('prettier-plugin-organize-imports')` does not work either
  //    because its dependency (ex. `typescript`) cannot be imported from
  //    monorepo root.

  // Normally, you'd just use 'prettier-plugin-organize-imports',
  // but it incorrectly looks for this relative to the monorepo prettier,
  // but we want it to find it in our workspace's node_modules.
  config.plugins = [path.join(__dirname, 'node_modules/prettier-plugin-organize-imports/index.js')];
}

module.exports = config;
