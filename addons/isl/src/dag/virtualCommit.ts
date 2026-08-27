/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../types';

import {t} from '../i18n';
import {DagCommitInfo} from './dagCommitInfo';

/**
 * The "wdir()" virtual hash.
 * This needs to match the CLI's interpretation of "wdir()". See `wdirhex` in sapling/node.py.
 */
export const WDIR_NODE = 'ffffffffffffffffffffffffffffffffffffffff';

export const YOU_ARE_HERE_VIRTUAL_COMMIT: DagCommitInfo = DagCommitInfo.fromCommitInfo({
  hash: WDIR_NODE,
  title: '',
  parents: [],
  phase: 'draft',
  isDot: false,
  date: new Date(8640000000000000),
  bookmarks: [],
  remoteBookmarks: [],
  author: '',
  description: t('You are here'),
  filePathsSample: [],
  totalFileCount: 0,
  isYouAreHere: true,
});

/**
 * Creates a virtual commit representing sibling worktree(s) checked out at `parentHash`.
 * Rendered as its own row directly above the real commit, mirroring `YOU_ARE_HERE_VIRTUAL_COMMIT`.
 */
export function makeCheckedOutElsewhereVirtualCommit(parentHash: Hash): DagCommitInfo {
  return DagCommitInfo.fromCommitInfo({
    hash: `checked-out-elsewhere:${parentHash}`,
    title: '',
    parents: [parentHash],
    phase: 'draft',
    isDot: false,
    // Slightly earlier than YOU_ARE_HERE_VIRTUAL_COMMIT's date so that if a sibling
    // is checked out at the same hash as your own dot, "You are here" sorts above
    // "checked out elsewhere".
    date: new Date(8640000000000000 - 1),
    bookmarks: [],
    remoteBookmarks: [],
    author: '',
    description: '',
    filePathsSample: [],
    totalFileCount: 0,
    isCheckedOutElsewhere: true,
  });
}
