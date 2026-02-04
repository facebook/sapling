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
  gitHubDiffNewCommentInputCallbacks,
  gitHubPullRequestLineToPositionForFile,
  gitHubPullRequestSelectedVersionIndex,
  gitHubPullRequestVersions,
  gitHubThreadsForDiffFile,
  nullAtom,
} from '../../recoil';
import {gitHubDiffCommitIDsAtom} from '../atoms';
import {useAtomValue} from 'jotai';
import {loadable} from 'jotai/utils';
import {useMemo} from 'react';
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
 * Uses a hybrid approach during migration:
 * - commitIDs comes from the Jotai gitHubDiffCommitIDsAtom
 * - Other selectors still use Recoil
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
  // Use Jotai for commitIDs (migrated atom)
  const loadableCommitIDsAtom = useMemo(() => loadable(gitHubDiffCommitIDsAtom), []);
  const commitIDsLoadable = useAtomValue(loadableCommitIDsAtom);

  // Use Recoil for remaining selectors that haven't been migrated yet
  const recoilLoadable = useRecoilValueLoadable(
    waitForAll([
      diffAndTokenize({path, before, after, scopeName, colorMode}),
      gitHubThreadsForDiffFile(path),
      gitHubDiffNewCommentInputCallbacks,
      isPullRequest ? gitHubPullRequestVersions : nullAtom,
      isPullRequest ? gitHubPullRequestSelectedVersionIndex : nullAtom,
      isPullRequest ? gitHubPullRequestLineToPositionForFile(path) : nullAtom,
    ]),
  );

  // Handle loading state from either source
  if (recoilLoadable.state === 'loading' || commitIDsLoadable.state === 'loading') {
    return {state: 'loading'};
  }

  // Handle error state from either source
  if (recoilLoadable.state === 'hasError') {
    return {state: 'hasError', error: recoilLoadable.contents as Error};
  }
  if (commitIDsLoadable.state === 'hasError') {
    return {state: 'hasError', error: commitIDsLoadable.error as Error};
  }

  const [diffAndTokenizeResult, threads, newCommentInputCallbacks] = recoilLoadable.contents;
  const commitIDs = commitIDsLoadable.data;

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
