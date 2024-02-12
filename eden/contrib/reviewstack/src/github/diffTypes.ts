/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {GitObjectID, TreeEntry} from './types';

/**
 * For the moment, list of paths that have been modified.
 */
export type Diff = CommitChange[];

export type DiffWithCommitIDs = {
  diff: Diff;
  commitIDs?: DiffCommitIDs | null;
};

/**
 * The commits whose trees were used to produce the diff.
 */
export type DiffCommitIDs = {
  before: GitObjectID;
  after: GitObjectID;
};

export type AddChange = {
  type: 'add';
  basePath: string;
  entry: TreeEntry;
};
export type RemoveChange = {
  type: 'remove';
  basePath: string;
  entry: TreeEntry;
};
export type ModifyChange = {type: 'modify'; basePath: string; before: TreeEntry; after: TreeEntry};

export type CommitChange = AddChange | RemoveChange | ModifyChange;
