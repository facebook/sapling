/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * Hook for loading SplitDiffView data, bridging Recoil state to a Jotai-style API.
 *
 * This hook wraps the Recoil selectors used by SplitDiffView and provides a
 * loadable-like interface that will eventually be migrated to pure Jotai.
 */

import type {DiffAndTokenizeResponse} from '../../diffServiceWorker';
import type {DiffSide} from '../../generated/graphql';
import type {DiffCommitIDs} from '../../github/diffTypes';
import type {GitHubPullRequestReviewThread} from '../../github/pullRequestTimelineTypes';
import type {NewCommentInputCallbacks} from '../../recoil';

import {diffAndTokenize} from '../../diffServiceClient';
import {
  gitHubDiffCommitIDs,
  gitHubDiffNewCommentInputCallbacks,
  gitHubPullRequestLineToPositionForFile,
  gitHubPullRequestSelectedVersionIndex,
  gitHubPullRequestVersions,
  gitHubThreadsForDiffFile,
  nullAtom,
} from '../../recoil';
import {useRecoilValueLoadable, waitForAll} from 'recoil';

export type SplitDiffViewLoadableState =
  | {state: 'loading'}
  | {state: 'hasError'; error: Error}
  | {
      state: 'hasValue';
      data: {
        diffAndTokenize: DiffAndTokenizeResponse;
        threads: {[key in DiffSide]: GitHubPullRequestReviewThread[]} | null;
        newCommentInputCallbacks: NewCommentInputCallbacks | null;
        commitIDs: DiffCommitIDs | null;
      };
    };

/**
 * A Jotai-style hook that wraps the Recoil selectors used by SplitDiffView.
 * This provides a migration path from useRecoilValueLoadable to a Jotai-compatible API.
 *
 * Returns a loadable-like object with state: 'loading' | 'hasError' | 'hasValue'
 */
export function useSplitDiffViewData(
  path: string,
  before: string | null,
  after: string | null,
  scopeName: string | null,
  colorMode: 'day' | 'night',
  isPullRequest: boolean,
): SplitDiffViewLoadableState {
  const loadable = useRecoilValueLoadable(
    waitForAll([
      diffAndTokenize({path, before, after, scopeName, colorMode}),
      gitHubThreadsForDiffFile(path),
      gitHubDiffNewCommentInputCallbacks,
      gitHubDiffCommitIDs,
      isPullRequest ? gitHubPullRequestVersions : nullAtom,
      isPullRequest ? gitHubPullRequestSelectedVersionIndex : nullAtom,
      isPullRequest ? gitHubPullRequestLineToPositionForFile(path) : nullAtom,
    ]),
  );

  if (loadable.state === 'loading') {
    return {state: 'loading'};
  }

  if (loadable.state === 'hasError') {
    return {state: 'hasError', error: loadable.contents as Error};
  }

  const [diffAndTokenizeResult, threads, newCommentInputCallbacks, commitIDs] = loadable.contents;

  return {
    state: 'hasValue',
    data: {
      diffAndTokenize: diffAndTokenizeResult,
      threads,
      newCommentInputCallbacks,
      commitIDs,
    },
  };
}
