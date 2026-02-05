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
  gitHubThreadsForDiffFile,
  nullAtom,
} from '../../recoil';
import {
  gitHubDiffCommitIDsAtom,
  gitHubPullRequestLineToPositionForFileAtom,
  gitHubPullRequestSelectedVersionIndexAtom,
  gitHubPullRequestVersionsAtom,
  gitHubThreadsForDiffFileAtom,
} from '../atoms';
import {useAtomValue, useSetAtom} from 'jotai';
import {loadable} from 'jotai/utils';
import {useEffect, useMemo} from 'react';
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
 * - threads are synced from Recoil to Jotai atomFamily per file path
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

  // Read Jotai atoms for versions (these are synced from Recoil in JotaiRecoilSync)
  // Note: These are read to ensure they're loaded, but not directly used in the return value
  const versions = useAtomValue(gitHubPullRequestVersionsAtom);
  useAtomValue(gitHubPullRequestSelectedVersionIndexAtom);

  // Jotai atom for threads for this specific file path
  const threadsAtom = useMemo(() => gitHubThreadsForDiffFileAtom(path), [path]);
  const setJotaiThreads = useSetAtom(threadsAtom);

  // Jotai atom for line-to-position mapping for this file path
  const lineToPositionAtom = useMemo(
    () => gitHubPullRequestLineToPositionForFileAtom(path),
    [path],
  );
  const setJotaiLineToPosition = useSetAtom(lineToPositionAtom);

  // Use Recoil for remaining selectors that haven't been migrated yet
  const recoilLoadable = useRecoilValueLoadable(
    waitForAll([
      diffAndTokenize({path, before, after, scopeName, colorMode}),
      gitHubThreadsForDiffFile(path),
      gitHubDiffNewCommentInputCallbacks,
      isPullRequest ? gitHubPullRequestLineToPositionForFile(path) : nullAtom,
    ]),
  );

  // Sync threads from Recoil to Jotai for this file path
  useEffect(() => {
    if (recoilLoadable.state === 'hasValue') {
      const [, recoilThreads, , lineToPositionForFile] = recoilLoadable.contents;
      setJotaiThreads(recoilThreads);
      setJotaiLineToPosition(lineToPositionForFile);
    }
  }, [recoilLoadable, setJotaiThreads, setJotaiLineToPosition]);

  // Handle loading state from either source
  // Also wait for versions to be loaded for PR case
  if (recoilLoadable.state === 'loading' || commitIDsLoadable.state === 'loading') {
    return {state: 'loading'};
  }

  // PR case: versions haven't been synced yet from Recoil, or commitIDs aren't yet available.
  // This handles the race condition where gitHubPullRequestComparableVersionsAtom
  // hasn't been synced from Recoil yet, causing gitHubDiffCommitIDsAtom to return null.
  if (
    isPullRequest &&
    (versions.length === 0 ||
      (commitIDsLoadable.state === 'hasData' && commitIDsLoadable.data == null))
  ) {
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
      // Use the threads from the Recoil loadable directly for now,
      // but they're also synced to the Jotai atom for future consumers
      threads,
      newCommentInputCallbacks,
      commitIDs,
    },
  };
}
