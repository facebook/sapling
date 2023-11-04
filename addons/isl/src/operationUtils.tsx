/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from './types';
import type {Snapshot} from 'recoil';

import {latestSuccessorUnlessExplicitlyObsolete} from './SuccessionTracker';
import {walkTreePostorder} from './getCommitTree';
import {AmendToOperation} from './operations/AmendToOperation';
import {uncommittedSelectionReadonly} from './partialSelection';
import {treeWithPreviews, uncommittedChangesWithPreviews} from './previews';

/**
 * Amend --to allows amending to a parent commit other than head.
 * Only allowed on a commit that is a parent of head, and when
 * your current selection is not a partial selection.
 */
export function isAmendToAllowedForCommit(commit: CommitInfo, snapshot: Snapshot): boolean {
  if (commit.isHead) {
    // no point, just amend normally
    return false;
  }

  const uncommittedChanges = snapshot.getLoadable(uncommittedChangesWithPreviews).valueMaybe();
  if (uncommittedChanges == null || uncommittedChanges.length === 0) {
    // nothing to amend
    return false;
  }

  // amend --to doesn't handle partial chunk selections, only entire files
  const selection = snapshot.getLoadable(uncommittedSelectionReadonly).valueOrThrow();
  const hasPartialSelection = selection.hasChunkSelection();

  if (hasPartialSelection) {
    return false;
  }

  const trees = snapshot.getLoadable(treeWithPreviews).valueMaybe();
  if (trees == null) {
    return false;
  }

  const {treeMap} = trees;
  const tree = treeMap.get(commit.hash);
  if (tree == null) {
    return false;
  }

  // to amend --to, you must select parent of the head commit
  for (const child of walkTreePostorder(tree.children)) {
    if (child.info.isHead) {
      // found the head commit
      return true;
    }
  }

  return false;
}

export function getAmendToOperation(commit: CommitInfo, snapshot: Snapshot): AmendToOperation {
  const selection = snapshot.getLoadable(uncommittedSelectionReadonly).valueOrThrow();
  const uncommittedChanges = snapshot.getLoadable(uncommittedChangesWithPreviews).valueOrThrow();

  const paths = uncommittedChanges
    .filter(change => selection.isFullySelected(change.path))
    .map(change => change.path);
  return new AmendToOperation(latestSuccessorUnlessExplicitlyObsolete(commit), paths);
}
