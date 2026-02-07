/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * Hook for loading SplitDiffView data using Jotai atoms.
 *
 * This hook is now fully migrated to Jotai. It no longer uses Recoil for
 * line-to-position mapping - that is now computed natively in Jotai via
 * gitHubPullRequestComputedLineToPositionForFileAtom.
 */

import type {DiffAndTokenizeResponse} from '../../diffServiceWorker';
import type {DiffCommitIDs} from '../../github/diffTypes';
import type {GitHubPullRequestReviewThread} from '../../github/pullRequestTimelineTypes';

import {diffAndTokenizeAtom} from '../../diffServiceClient';
import {DiffSide} from '../../generated/graphql';
import {
  gitHubDiffCommitIDsAtom,
  gitHubPullRequestAtom,
  gitHubPullRequestCanAddCommentAtom,
  gitHubPullRequestComparableVersionsAtom,
  gitHubPullRequestComputedLineToPositionForFileAtom,
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
 * A fully Jotai-based hook for loading SplitDiffView data.
 *
 * All data sources are now Jotai atoms:
 * - commitIDs comes from gitHubDiffCommitIDsAtom
 * - threads are computed natively in Jotai via gitHubThreadsForDiffFileAtom
 * - Comment input callbacks use Jotai atoms
 * - diffAndTokenize uses the Jotai diffAndTokenizeAtom
 * - lineToPosition uses gitHubPullRequestComputedLineToPositionForFileAtom
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

  // Read Jotai atoms for versions - now computed natively in Jotai
  // Note: These are read to ensure they're loaded, but not directly used in the return value
  // We use loadable to avoid suspending the whole component
  const loadableVersionsAtom = useMemo(() => loadable(gitHubPullRequestVersionsAtom), []);
  const versionsLoadable = useAtomValue(loadableVersionsAtom);
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
        // Only check if versions are loaded
        if (versionsLoadable.state === 'hasData') {
          const versions = versionsLoadable.data;
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
        }
        return;
      }

      setCellAtom({path, lineNumber, side});
    },
    [store, setCellAtom, setNotification, versionsLoadable],
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

  // Use Jotai for computed lineToPosition (fully migrated from Recoil)
  const computedLineToPositionLoadableAtom = useMemo(
    () => loadable(gitHubPullRequestComputedLineToPositionForFileAtom(path)),
    [path],
  );
  const lineToPositionLoadable = useAtomValue(computedLineToPositionLoadableAtom);

  // Sync the computed lineToPosition to the writable atom for other consumers
  useEffect(() => {
    if (isPullRequest && lineToPositionLoadable.state === 'hasData') {
      setJotaiLineToPosition(lineToPositionLoadable.data);
    }
  }, [isPullRequest, lineToPositionLoadable, setJotaiLineToPosition]);

  // Handle loading state from all sources
  // Also wait for versions to be loaded for PR case
  if (
    commitIDsLoadable.state === 'loading' ||
    diffAndTokenizeLoadable.state === 'loading' ||
    (isPullRequest && lineToPositionLoadable.state === 'loading')
  ) {
    return {state: 'loading'};
  }

  // PR case: versions haven't loaded yet, or commitIDs aren't yet available.
  // This handles the race condition where versions are still loading.
  if (
    isPullRequest &&
    (versionsLoadable.state !== 'hasData' ||
      versionsLoadable.data.length === 0 ||
      (commitIDsLoadable.state === 'hasData' && commitIDsLoadable.data == null))
  ) {
    return {state: 'loading'};
  }

  // Handle error state from all sources
  if (commitIDsLoadable.state === 'hasError') {
    return {state: 'hasError', error: commitIDsLoadable.error as Error};
  }
  if (diffAndTokenizeLoadable.state === 'hasError') {
    return {state: 'hasError', error: diffAndTokenizeLoadable.error as Error};
  }
  if (isPullRequest && lineToPositionLoadable.state === 'hasError') {
    return {state: 'hasError', error: lineToPositionLoadable.error as Error};
  }
  if (isPullRequest && versionsLoadable.state === 'hasError') {
    return {state: 'hasError', error: versionsLoadable.error as Error};
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
