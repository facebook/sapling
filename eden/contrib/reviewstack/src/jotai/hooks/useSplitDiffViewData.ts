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
import type {DiffCommitIDs} from '../../github/diffTypes';
import type {GitHubPullRequestReviewThread} from '../../github/pullRequestTimelineTypes';

import {diffAndTokenizeAtom} from '../../diffServiceClient';
import {DiffSide} from '../../generated/graphql';
import {
  gitHubPullRequestLineToPositionForFile,
  nullAtom,
} from '../../recoil';
import {
  gitHubDiffCommitIDsAtom,
  gitHubPullRequestAtom,
  gitHubPullRequestCanAddCommentAtom,
  gitHubPullRequestComparableVersionsAtom,
  gitHubPullRequestLineToPositionForFileAtom,
  gitHubPullRequestNewCommentInputCellAtom,
  gitHubPullRequestSelectedVersionIndexAtom,
  gitHubPullRequestVersionsAtom,
  gitHubThreadsForDiffFileAtom,
  notificationMessageAtom,
} from '../atoms';
import {useAtomValue, useSetAtom, useStore} from 'jotai';
import {loadable} from 'jotai/utils';
import {useCallback, useEffect, useMemo} from 'react';
import {useRecoilValueLoadable} from 'recoil';

/**
 * Type for the new comment input callbacks.
 * Migrated from NewCommentInputCallbacks in recoil.ts
 */
export type NewCommentInputCallbacks = {
  onShowNewCommentInput: (event: React.MouseEvent<HTMLTableElement>) => void;
  onResetNewCommentInput: () => void;
};

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
 * - threads are now computed natively in Jotai
 * - Comment input callbacks are now Jotai-based
 * - diffAndTokenize is now a Jotai atom (diffAndTokenizeAtom)
 * - lineToPosition still uses Recoil (complex dependency chain)
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

  // Jotai atom for threads - now computed natively in Jotai
  const threadsAtom = useMemo(() => gitHubThreadsForDiffFileAtom(path), [path]);
  const threads = useAtomValue(threadsAtom);

  // Jotai atom for line-to-position mapping for this file path
  const lineToPositionAtom = useMemo(
    () => gitHubPullRequestLineToPositionForFileAtom(path),
    [path],
  );
  const setJotaiLineToPosition = useSetAtom(lineToPositionAtom);

  // Jotai-based comment input callbacks
  const store = useStore();
  const pullRequest = useAtomValue(gitHubPullRequestAtom);
  const setCellAtom = useSetAtom(gitHubPullRequestNewCommentInputCellAtom);
  const setNotification = useSetAtom(notificationMessageAtom);

  const onShowNewCommentInput = useCallback(
    (event: React.MouseEvent<HTMLTableElement>) => {
      const {target} = event;
      if (!(target instanceof HTMLTableCellElement)) {
        return;
      }

      const {lineNumber: lineNumberStr, path, side: sideStr} = target.dataset;
      if (lineNumberStr == null || path == null || sideStr == null) {
        return;
      }

      const lineNumber = parseInt(lineNumberStr, 10);
      const side =
        sideStr === DiffSide.Left
          ? DiffSide.Left
          : sideStr === DiffSide.Right
            ? DiffSide.Right
            : null;
      if (isNaN(lineNumber) || side == null) {
        return;
      }

      // Check if we can add a comment using the Jotai atom
      const canAddComment = store.get(
        gitHubPullRequestCanAddCommentAtom({lineNumber, path, side}),
      );
      if (!canAddComment) {
        // Check why we can't add a comment and show appropriate message
        const versions = store.get(gitHubPullRequestVersionsAtom);
        const selectedVersionIndex = store.get(gitHubPullRequestSelectedVersionIndexAtom);
        const comparableVersions = store.get(gitHubPullRequestComparableVersionsAtom);

        if (selectedVersionIndex !== versions.length - 1) {
          setNotification({
            type: 'info',
            message:
              'Comments can only be added when viewing the latest version of the pull request.',
          });
        } else if (comparableVersions?.beforeCommitID != null && side === DiffSide.Left) {
          setNotification({
            type: 'info',
            message:
              'Comments cannot be added to the left side when comparing versions. The left side shows an older revision that is no longer part of the pull request.',
          });
        }
        return;
      }

      setCellAtom({path, lineNumber, side});
    },
    [store, setCellAtom, setNotification],
  );

  const onResetNewCommentInput = useCallback(() => {
    setCellAtom(null);
  }, [setCellAtom]);

  const newCommentInputCallbacks: NewCommentInputCallbacks | null = useMemo(() => {
    if (pullRequest != null) {
      return {onShowNewCommentInput, onResetNewCommentInput};
    }
    return null;
  }, [pullRequest, onShowNewCommentInput, onResetNewCommentInput]);

  // Use Jotai for diffAndTokenize (migrated from Recoil)
  const diffAndTokenizeParams = useMemo(
    () => ({path, before, after, scopeName, colorMode}),
    [path, before, after, scopeName, colorMode],
  );
  const diffAndTokenizeLoadableAtom = useMemo(
    () => loadable(diffAndTokenizeAtom(diffAndTokenizeParams)),
    [diffAndTokenizeParams],
  );
  const diffAndTokenizeLoadable = useAtomValue(diffAndTokenizeLoadableAtom);

  // Use Recoil for lineToPosition (still depends on Recoil selectors)
  const recoilLoadable = useRecoilValueLoadable(
    isPullRequest ? gitHubPullRequestLineToPositionForFile(path) : nullAtom,
  );

  // Sync lineToPosition from Recoil to Jotai for this file path
  useEffect(() => {
    if (recoilLoadable.state === 'hasValue') {
      setJotaiLineToPosition(recoilLoadable.contents);
    }
  }, [recoilLoadable, setJotaiLineToPosition]);

  // Handle loading state from either source
  // Also wait for versions to be loaded for PR case
  if (
    recoilLoadable.state === 'loading' ||
    commitIDsLoadable.state === 'loading' ||
    diffAndTokenizeLoadable.state === 'loading'
  ) {
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
  if (diffAndTokenizeLoadable.state === 'hasError') {
    return {state: 'hasError', error: diffAndTokenizeLoadable.error as Error};
  }

  const diffAndTokenizeResult = diffAndTokenizeLoadable.data;
  const commitIDs = commitIDsLoadable.data;

  return {
    state: 'hasValue',
    data: {
      diffAndTokenize: diffAndTokenizeResult,
      // threads are now computed natively in Jotai
      threads,
      newCommentInputCallbacks,
      commitIDs,
    },
  };
}
