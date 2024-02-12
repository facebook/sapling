/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

const SELECTOR = `
   VariableDeclarator
   > CallExpression:matches(
     [callee.name="atom"],
     [callee.name="atomFamily"],
     [callee.name="selector"],
     [callee.name="selectorFamily"]
   )
   > ObjectExpression
   > Property[key.name="key"]
   > Literal
 `.replaceAll('\n', ' ');

module.exports = {
  create(context) {
    return {
      [SELECTOR](node) {
        const keyName = node.value;
        const ancestors = context.getAncestors();
        const variable = ancestors.find(({type}) => type === 'VariableDeclarator');
        const variableName = variable.id.name;

        if (keyName !== variableName) {
          context.report({
            node,
            message: 'Recoil key should match variable name',
          });
        }
      },
    };
  },
};
