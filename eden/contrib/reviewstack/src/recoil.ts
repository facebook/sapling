/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PullRequest} from './github/pullRequestTimelineTypes';
import type {GitObjectID} from './github/types';

import {atom} from 'recoil';

// Internal type - exported from jotai/atoms.ts for component consumers.
type GitHubOrgAndRepo = {
  org: string;
  repo: string;
};

export const gitHubOrgAndRepo = atom<GitHubOrgAndRepo | null>({
  key: 'gitHubOrgAndRepo',
  default: null,
});

export const gitHubPullRequest = atom<PullRequest | null>({
  key: 'gitHubPullRequest',
  default: null,
});

/**
 * The selected version index. This is now a simple atom - the default value
 * is computed and synced from Jotai via JotaiRecoilSync.
 */
export const gitHubPullRequestSelectedVersionIndex = atom<number>({
  key: 'gitHubPullRequestSelectedVersionIndex',
  default: 0,
});

/**
 * When there is no "before" explicitly selected, the view shows the Diff for
 * the selected "after" version compared to its parent.
 * Internal type - exported from jotai/atoms.ts for component consumers.
 *
 * This is now a simple atom - the default value is computed and synced from
 * Jotai via JotaiRecoilSync.
 */
type ComparableVersions = {
  beforeCommitID: GitObjectID | null;
  afterCommitID: GitObjectID;
};

export const gitHubPullRequestComparableVersions = atom<ComparableVersions>({
  key: 'gitHubPullRequestComparableVersions',
  default: {
    beforeCommitID: null,
    afterCommitID: '' as GitObjectID,
  },
});
