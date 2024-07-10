/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Dag} from './dag/dag';
import type {ExactRevset, OptimisticRevset} from './types';

import {
  exactRevset,
  optimisticRevset,
  succeedableRevset,
  type CommitInfo,
  type SucceedableRevset,
} from './types';

/**
 * Get the latest successor hash of the given hash,
 * traversing multiple successions if necessary.
 * Returns original hash if no successors were found.
 *
 * Useful for previews to ensure they're working with the latest version of a commit,
 * given that they may have been queued up while another operation ran and eventually caused succession.
 *
 * Note: if an ExactRevset is passed, don't look up the successor.
 */
export function latestSuccessor(
  ctx: Dag,
  oldRevset: SucceedableRevset | ExactRevset | OptimisticRevset,
): string {
  let hash = oldRevset.type === 'optimistic-revset' ? oldRevset.fake : oldRevset.revset;
  if (oldRevset.type === 'exact-revset') {
    return hash;
  }
  hash = ctx.followSuccessors(hash).toHashes().first() ?? hash;
  return hash;
}

/**
 * Typically we want to use succeedable revsets everywhere, to maximize support for queued commands.
 * But if you see and act on a visibly obsolete commit in the UI, we should use its exact hash,
 * so that you don't suddenly act on a seemingly unrelated commit.
 */
export function latestSuccessorUnlessExplicitlyObsolete(
  commit: Readonly<CommitInfo>,
): SucceedableRevset | ExactRevset | OptimisticRevset {
  if (commit.optimisticRevset != null) {
    return optimisticRevset(commit.optimisticRevset, commit.hash);
  }
  if (commit.successorInfo?.type != null) {
    return exactRevset(commit.hash);
  }
  return succeedableRevset(commit.hash);
}
