/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Commit, GitObjectID} from './github/types';

export type CommitDiffPair = {
  baseCommitID: GitObjectID;
  commitID: GitObjectID;
};

/**
 * Return the two isolated commit changes used to compare Sapling versions.
 * Each submitted Sapling commit can be rebased when a lower stack entry
 * changes, so comparing the version heads directly also includes that parent
 * change.
 */
export default function saplingVersionDiffPairs(
  beforeCommit: Pick<Commit, 'oid' | 'parents'> | null,
  afterCommit: Pick<Commit, 'oid' | 'parents'>,
): {before: CommitDiffPair | null; after: CommitDiffPair} | null {
  if (afterCommit.parents.length !== 1) {
    return null;
  }
  const after = {
    baseCommitID: afterCommit.parents[0],
    commitID: afterCommit.oid,
  };
  if (beforeCommit == null) {
    return {before: null, after};
  }
  if (beforeCommit.parents.length !== 1) {
    return null;
  }
  return {
    before: {
      baseCommitID: beforeCommit.parents[0],
      commitID: beforeCommit.oid,
    },
    after,
  };
}
