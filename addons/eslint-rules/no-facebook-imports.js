/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

const path = require('path');

module.exports = {
  meta: {
    type: 'problem',
    docs: {
      description: 'disallow imports from facebook paths in non-facebook files',
    },
    fixable: null, // Not automatically fixable
    messages: {
      noFacebookImports:
        'Imports from facebook paths are only allowed in files inside facebook folders or files named "InternalImports".',
    },
    schema: [], // no options
  },
  create(context) {
    return {
      ImportDeclaration(node) {
        const importPath = node.source.value;

        // Check if the import path matches .*/facebook/.*
        if (!/.*\/facebook\/.*/.test(importPath)) {
          return;
        }

        // Get the current file path
        const filename = context.getFilename();
        const relativePath = path.relative(process.cwd(), filename);

        // Extract the file name without extension
        const baseName = path.basename(filename, path.extname(filename));

        // Check if the file is named "InternalImports"
        if (
          baseName === 'InternalImports' ||
          baseName === 'Internal' ||
          baseName === 'InternalTypes'
        ) {
          return;
        }

        // Check if the file is inside a facebook folder
        const pathParts = relativePath.split(path.sep);
        if (pathParts.includes('facebook')) {
          return;
        }

        // Report the violation
        context.report({
          node,
          messageId: 'noFacebookImports',
        });
      },
    };
  },
};
