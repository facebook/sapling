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
      description:
        'Require explicit type annotations for callback parameters in Internal API promise chains (.then and .catch)',
    },
    fixable: null, // Not automatically fixable
    messages: {
      missingTypeAnnotation:
        'Parameters in callbacks for Internal API promise chains must have explicit type annotations to avoid "any" type errors when mirrored to open source.',
    },
    schema: [], // no options
  },
  create(context) {
    // Helper function to check if a node is part of an Internal API call chain
    function isInternalApiCall(node) {
      // Start from the object of the .then() call and traverse up
      let current = node;

      while (current) {
        // Check for direct Internal.something pattern
        if (
          current.type === 'MemberExpression' &&
          current.object &&
          current.object.type === 'Identifier' &&
          current.object.name === 'Internal'
        ) {
          return true;
        }

        // Check for property access on Internal
        if (current.type === 'MemberExpression' && current.object) {
          current = current.object;
          continue;
        }

        // Check for call expressions
        if (current.type === 'CallExpression' && current.callee) {
          current = current.callee;
          continue;
        }

        // Check for optional chaining
        if (current.type === 'ChainExpression' && current.expression) {
          current = current.expression;
          continue;
        }

        // If we can't traverse further up, break the loop
        break;
      }

      return false;
    }

    return {
      // Look for .then() and .catch() calls
      'CallExpression[callee.property.name="then"], CallExpression[callee.property.name="catch"]'(
        node,
      ) {
        const methodName = node.callee.property.name;

        // Check if the object of the .then() or .catch() call is part of an Internal API call chain
        if (!isInternalApiCall(node.callee.object)) {
          return;
        }

        // Check if there are arguments to the call
        if (!node.arguments || node.arguments.length === 0) {
          return;
        }

        // Get the callback function (first argument to .then() or .catch())
        const callback = node.arguments[0];

        // Check if it's an arrow function or function expression
        if (callback.type !== 'ArrowFunctionExpression' && callback.type !== 'FunctionExpression') {
          return;
        }

        // Check each parameter for type annotations
        const params = callback.params || [];
        for (const param of params) {
          // If the parameter doesn't have a type annotation, report an error
          if (!param.typeAnnotation) {
            context.report({
              node: param,
              messageId: 'missingTypeAnnotation',
            });
          }
        }
      },
    };
  },
};
