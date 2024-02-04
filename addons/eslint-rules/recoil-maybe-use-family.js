/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

const containerMethods = new Set(['get', 'has']);

/**
 * Example:
 *
 *   function MaybeBackground(props: {id: string}) {
 *     const set = useRecoilValue(shouldUseBackgroundAtom);
 *     const hasBackground = set.has(props.id)
 *     return hasBackground ? <Background /> : null;
 *   }
 *
 * will trigger re-render of all MaybeBackground once the atom has any small
 * changes. To only re-render changed items, use a selectorFamily:
 *
 *   const shouldUseBackgroundById = selectorFamily({
 *     key: 'shouldUseBackgroundById',
 *     get: (id) => ({get}) => get(shouldUseBackgroundAtom).has(id),
 *   })
 *
 *   function MaybeBackground(props: {id: string}) {
 *     const hasBackground = useRecoilValue(shouldUseBackgroundById(props.id);
 *     return hasBackground ? <Background /> : null;
 *   }
 */
module.exports = {
  meta: {
    type: 'problem',
    docs: {
      description: 'Suggest selectorFamily for container-get-key patterns to avoid re-render.',
    },
  },
  create(context) {
    return {
      VariableDeclarator(node) {
        if (node.init?.type === 'CallExpression' && node.init.callee.name === 'useRecoilValue') {
          analyzeUseRecoilValue(node, context);
        }
      },
    };
  },
};

function analyzeUseRecoilValue(node, context) {
  const varName = node.id.name;
  // Analyze references to this variable.
  const sourceCode = context.sourceCode ?? context.getSourceCode();
  const scope = sourceCode.getScope?.(node) ?? context.getScope();
  // The container method being used: "get" or "has".
  let method = null;
  // Find references of this variable.
  const references = scope.variables.find(({name}) => name === varName)?.references ?? [];
  // Check the references. The first one is the declaraction and should be skipped.
  for (const reference of references.slice(1)) {
    // Inside a loop for potentially legit usecase. Allow.
    const innerScope = reference.from;
    if (innerScope?.block.type === 'ArrowFunctionExpression') {
      return;
    }
    let nextNode = sourceCode.getTokenAfter(reference.identifier);
    // Container methods like ".get" or ".has"?
    let currentMethod = null;
    if (nextNode?.type === 'Punctuator' && (nextNode.value === '.' || nextNode.value === '?.')) {
      nextNode = sourceCode.getTokenAfter(nextNode);
      if (nextNode?.type === 'Identifier' && containerMethods.has(nextNode.value)) {
        currentMethod = method = nextNode.value;
      }
    }
    // Called other methods, or have other use-cases. Allow.
    if (currentMethod === null) {
      return;
    }
  }
  if (method !== null) {
    context.report({
      node,
      message:
        'Recoil value `{{ varName }}` seems to be only used for `{{ method }}`. Consider moving `{{ method }}` to a `selectorFamily` to avoid re-render.',
      data: {
        varName,
        method,
      },
    });
  }
}
