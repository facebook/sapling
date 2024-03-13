/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from './types';

import {latestSuccessorUnlessExplicitlyObsolete} from './SuccessionTracker';
import {readAtom} from './jotaiUtils';
import {AmendToOperation} from './operations/AmendToOperation';
import {uncommittedSelection} from './partialSelection';
import {dagWithPreviews, uncommittedChangesWithPreviews} from './previews';

/**
 * Amend --to allows amending to a parent commit other than head.
 * Only allowed on a commit that is a parent of head, and when
 * your current selection is not a partial selection.
 */
export function isAmendToAllowedForCommit(commit: CommitInfo): boolean {
  if (commit.isDot || commit.phase === 'public' || commit.successorInfo != null) {
    // no point, just amend normally
    return false;
  }

  const uncommittedChanges = readAtom(uncommittedChangesWithPreviews);
  if (uncommittedChanges == null || uncommittedChanges.length === 0) {
    // nothing to amend
    return false;
  }

  // amend --to doesn't handle partial chunk selections, only entire files
  const selection = readAtom(uncommittedSelection);
  const hasPartialSelection = selection.hasChunkSelection();

  if (hasPartialSelection) {
    return false;
  }

  const dag = readAtom(dagWithPreviews);
  const head = dag?.resolve('.');
  if (dag == null || head == null || !dag.has(commit.hash)) {
    return false;
  }

  return dag.isAncestor(commit.hash, head.hash);
}

export function getAmendToOperation(commit: CommitInfo): AmendToOperation {
  const selection = readAtom(uncommittedSelection);
  const uncommittedChanges = readAtom(uncommittedChangesWithPreviews);

  const paths = uncommittedChanges
    .filter(change => selection.isFullySelected(change.path))
    .map(change => change.path);
  return new AmendToOperation(latestSuccessorUnlessExplicitlyObsolete(commit), paths);
}
