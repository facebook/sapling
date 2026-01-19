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
 *     function MaybeHighlight(props: {id: string}) {
 *       const set = useAtomValue(selectedSet);
 *       const selected = set.has(props.id)
 *       return selected ? <Highlight /> : null;
 *     }
 *
 * will trigger re-render of all MaybeHigh once the atom has any small
 * changes. To only re-render changed items, use `atomFamilyWeak`:
 *
 *     const selectedById = atomFamilyWeak((id: string) => {
 *       return atom(get => get(selectedsSet).has(id));
 *     });
 *     function MaybeHighlight(props: {id: string}) {
 *       const selected = useAtomValue(selectedById(props.id));
 *       ...
 *     }
 *
 * Alternatively, calculate a memo-ed atom on demand:
 *
 *     function MaybeHighlight({id}: {id: string}) {
 *       const selectedAtom = useMemo(() => atom(get => get(selectedsSet).has(id)), [id]);
 *       const selected = useAtomValue(selectedAtom);
 *       ...
 *     }
 *
 * The `atomFamilyWeak` might keep some extra states alive to satisfy other
 * use-cases. The memo-ed atom approach has no memory leak and might be
 * preferred if there are only 1 component that needs this derived atom state.
 */
module.exports = {
  meta: {
    type: 'problem',
    docs: {
      description: 'Suggest alternatives for container-get-key patterns to avoid re-render.',
    },
  },
  create(context) {
    return {
      VariableDeclarator(node) {
        if (node.init?.type === 'CallExpression' && node.init.callee.name === 'useAtomValue') {
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
  // Check the references. The first one is the declaration and should be skipped.
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
        'Atom value `{{ varName }}` seems to be only used for `{{ method }}`. Consider moving `{{ method }}` to a `atomFamilyWeak` or use `{{ useMethod }}` to avoid re-render.',
      data: {
        varName,
        method,
        useMethod: method === 'get' ? 'useAtomGet' : 'useAtomHas',
      },
    });
  }
}
