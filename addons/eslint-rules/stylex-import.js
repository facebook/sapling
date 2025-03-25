/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

module.exports = {
  meta: {
    type: 'problem',
    docs: {
      description: 'disallow default import of stylex',
    },
    fixable: 'code', // This indicates that the rule is fixable
    messages: {
      noDefaultStylexImport:
        "Use `import * as stylex from '@stylexjs/stylex'` instead of default import to avoid test breakages.",
    },
    schema: [], // no options
  },
  create(context) {
    return {
      ImportDeclaration(node) {
        if (
          node.source.value === '@stylexjs/stylex' &&
          node.specifiers.some(specifier => specifier.type === 'ImportDefaultSpecifier')
        ) {
          context.report({
            node,
            messageId: 'noDefaultStylexImport',
            fix(fixer) {
              // Construct the correct import statement
              const importStatement = `import * as stylex from '@stylexjs/stylex';`;
              return fixer.replaceText(node, importStatement);
            },
          });
        }
      },
    };
  },
};
