/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitTree} from './getCommitTree';
import type {CommitInfo, Hash} from './types';

import {firstOfIterable} from './utils';

/**
 * Given a set of selected commits, order them into an array from bottom to top.
 * If commits are not contiguous, returns undefined.
 * This selection must be linear and contiguous: no branches out are allowed.
 * This constitutes a set of commits that may be "folded"/combined into a single commit via `sl fold`.
 */
export function getFoldableRange(
  selection: Set<string>,
  treeMap: Map<string, CommitTree>,
): Array<CommitInfo> | undefined {
  const contiguous = getOrderedContiguousSelection(selection, treeMap);
  if (contiguous == null || contiguous.length <= 1) {
    return undefined;
  }
  return contiguous?.map(tree => tree.info);
}

function getOrderedContiguousSelection(
  selection: Set<string>,
  treeMap: Map<string, CommitTree>,
): Array<CommitTree> | undefined {
  const bottomMost = bottomMostOfSelection(selection, treeMap);

  if (bottomMost == null) {
    return undefined;
  }

  // Starting from the bottom, walk up children as long as they're all in the selection,
  // to form the range.
  // Validate invariants along the way to ensure the selection is valid for folding.

  const stack: Array<CommitTree> = [];
  let current = treeMap.get(bottomMost);
  while (current != null) {
    if (!selection.has(current.info.hash)) {
      // Once we find a commit outside our selection, we've reached the end.
      break;
    }

    // Must be linear
    if (
      current.children.length !== 1 &&
      // ...except the topmost commit may have as many children as it likes
      stack.length !== selection.size - 1
    ) {
      return undefined;
    }

    // Public commits may not be folded
    if (current.info.phase === 'public') {
      return undefined;
    }

    stack.push(current);
    current = current.children[0];
  }

  if (stack.length !== selection.size) {
    return undefined;
  }

  return stack;
}

/**
 * Given a bag of unordered commits that ostensibly belong to a contiguous selection,
 * find the bottom-most commit.
 */
function bottomMostOfSelection(
  selection: Set<string>,
  treeMap: Map<string, CommitTree>,
): Hash | undefined {
  // Start from any commit as the "base".
  // Navigate up parents until a public commit is reached.
  // Any time a draft commit that's in the selection is encountered, use that as the new base.
  // This will give the bottom-most commit.
  let baseHash = firstOfIterable(selection.values());
  if (baseHash == null) {
    return undefined;
  }
  let current = treeMap.get(baseHash);
  while (current != null) {
    if (current.info.phase === 'public') {
      break;
    }
    if (selection.has(current.info.hash)) {
      baseHash = current.info.hash;
    }

    current = treeMap.get(current.info.parents[0]);
  }

  return baseHash;
}
