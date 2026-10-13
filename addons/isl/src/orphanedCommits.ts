/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Dag} from './dag/dag';
import type {Hash} from './types';

export type OrphanedRebaseEntry = {orphanedChild: Hash; successorHash: Hash; isLanded: boolean};

export type OrphanedStackInfo = {
  rebaseEntries: Array<OrphanedRebaseEntry>;
  allOrphaned: Array<Hash>;
  allSuccessors: Array<Hash>;
  isLanded: boolean;
};

/**
 * For a given obsolete commit, returns info about its orphaned (non-obsolete)
 * children and the latest successor to rebase them onto, or null if the button
 * should not be shown.
 */
function orphanedChildrenForCommit(dag: Dag, hash: Hash) {
  const commit = dag.get(hash);
  if (commit == null || commit.successorInfo == null) {
    return null;
  }

  const successorType = commit.successorInfo.type;

  // Find non-obsolete children of this obsolete commit. Checked before following successors
  // because most obsolete commits in a stack have only obsolete children.
  const children = dag.children(hash);
  const nonObsoleteChildren = dag.nonObsolete(children);
  if (nonObsoleteChildren.size === 0) {
    return null;
  }

  // Follow the successor chain to find the latest non-obsolete successor.
  const successors = dag.followSuccessors(hash);
  // followSuccessors returns a set; remove the original hash to get just successors.
  const successorHashes = successors
    .toHashes()
    .toArray()
    .filter(h => h !== hash);
  if (successorHashes.length === 0) {
    return null;
  }

  // Pick the first successor. If it's not in the DAG or is itself obsolete, bail out.
  const successorHash = successorHashes[0];
  const successorCommit = dag.get(successorHash);
  if (successorCommit == null || successorCommit.successorInfo != null) {
    return null;
  }

  return {
    orphanedChildren: nonObsoleteChildren.toHashes().toArray(),
    successorHash,
    isLanded: successorType === 'land' || successorType === 'pushrebase',
  };
}

/**
 * For a given stack root hash, aggregates all orphaned children across all
 * obsolete commits in the stack. Returns the list of rebase operations needed
 * (each mapping an orphaned child to its target successor), or null if no
 * orphaned commits exist in the stack.
 */
export function orphanedChildrenInStack(dag: Dag, hash: Hash): OrphanedStackInfo | null {
  const stackHashes = dag.descendants(hash).toHashes().toArray();

  const rebaseEntries: Array<OrphanedRebaseEntry> = [];
  const allOrphaned: Hash[] = [];
  const allSuccessors: Hash[] = [];

  for (const h of stackHashes) {
    const info = orphanedChildrenForCommit(dag, h);
    if (info != null) {
      for (const child of info.orphanedChildren) {
        rebaseEntries.push({
          orphanedChild: child,
          successorHash: info.successorHash,
          isLanded: info.isLanded,
        });
        allOrphaned.push(child);
      }
      if (!allSuccessors.includes(info.successorHash)) {
        allSuccessors.push(info.successorHash);
      }
    }
  }

  if (rebaseEntries.length === 0) {
    return null;
  }

  return {
    rebaseEntries,
    allOrphaned,
    allSuccessors,
    isLanded: rebaseEntries.every(e => e.isLanded),
  };
}
