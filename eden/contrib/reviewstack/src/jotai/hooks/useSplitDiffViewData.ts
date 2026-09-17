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
import {reviewCommentRangeAtom} from '../../reviewCommentRange';
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
import {useCallback, useEffect, useMemo, useRef} from 'react';

/**
 * Type for the new comment input callbacks.
 */
export type NewCommentInputCallbacks = {
  onShowNewCommentInput: (event: React.MouseEvent<HTMLTableElement>) => void;
  onStartNewCommentRange: (event: React.PointerEvent<HTMLTableElement>) => void;
  onExtendNewCommentRange: (event: React.PointerEvent<HTMLTableElement>) => void;
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
  const setCommentRange = useSetAtom(reviewCommentRangeAtom);
  const dragSelectionActive = useRef(false);
  const suppressNextClick = useRef(false);

  const selectCommentLine = useCallback(
    (
      table: HTMLTableElement,
      target: EventTarget | null,
      extendFromAnchor: boolean,
      showCommentInput: boolean,
    ): boolean => {
      if (!(target instanceof HTMLTableCellElement)) {
        return false;
      }

      const {lineNumber: lineNumberStr, path, side: sideStr} = target.dataset;
      if (lineNumberStr == null || path == null || sideStr == null) {
        return false;
      }

      const lineNumber = parseInt(lineNumberStr, 10);
      const side =
        sideStr === DiffSide.Left
          ? DiffSide.Left
          : sideStr === DiffSide.Right
          ? DiffSide.Right
          : null;
      if (isNaN(lineNumber) || side == null) {
        return false;
      }

      // Check if we can add a comment using the Jotai atom
      const canAddComment = store.get(gitHubPullRequestCanAddCommentAtom({lineNumber, path, side}));
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
        return false;
      }

      const currentRange = store.get(reviewCommentRangeAtom);
      if (
        extendFromAnchor &&
        currentRange != null &&
        currentRange.path === path &&
        currentRange.side === side
      ) {
        const startLine = Math.min(currentRange.anchorLine, lineNumber);
        const endLine = Math.max(currentRange.anchorLine, lineNumber);
        const availableLines = new Set(
          Array.from(table.querySelectorAll<HTMLTableCellElement>('td.lineNumber'))
            .filter(cell => cell.dataset.path === path && cell.dataset.side === side)
            .map(cell => Number(cell.dataset.lineNumber)),
        );
        const rangeIsVisible = Array.from(
          {length: endLine - startLine + 1},
          (_, index) => startLine + index,
        ).every(selectedLine => availableLines.has(selectedLine));
        if (!rangeIsVisible) {
          setNotification({
            type: 'info',
            message:
              'A multi-line comment must cover contiguous visible lines on the same side of the diff.',
          });
          return false;
        }
        setCommentRange({...currentRange, startLine, endLine});
        setCellAtom(showCommentInput ? {path, lineNumber: endLine, side} : null);
        return true;
      }

      setCommentRange({
        anchorLine: lineNumber,
        startLine: lineNumber,
        endLine: lineNumber,
        path,
        side,
      });
      setCellAtom(showCommentInput ? {path, lineNumber, side} : null);
      return true;
    },
    [store, setCellAtom, setCommentRange, setNotification, versionsLoadable],
  );

  const finishDragSelection = useCallback(() => {
    if (!dragSelectionActive.current) {
      return;
    }
    dragSelectionActive.current = false;
    const range = store.get(reviewCommentRangeAtom);
    if (range != null) {
      setCellAtom({path: range.path, lineNumber: range.endLine, side: range.side});
    }
  }, [setCellAtom, store]);

  useEffect(() => {
    window.addEventListener('pointerup', finishDragSelection);
    window.addEventListener('pointercancel', finishDragSelection);
    return () => {
      window.removeEventListener('pointerup', finishDragSelection);
      window.removeEventListener('pointercancel', finishDragSelection);
    };
  }, [finishDragSelection]);

  const onStartNewCommentRange = useCallback(
    (event: React.PointerEvent<HTMLTableElement>) => {
      if (event.button !== 0) {
        return;
      }
      if (selectCommentLine(event.currentTarget, event.target, event.shiftKey, false)) {
        event.preventDefault();
        dragSelectionActive.current = true;
        suppressNextClick.current = true;
      }
    },
    [selectCommentLine],
  );

  const onExtendNewCommentRange = useCallback(
    (event: React.PointerEvent<HTMLTableElement>) => {
      if (!dragSelectionActive.current || event.buttons === 0) {
        return;
      }
      if (selectCommentLine(event.currentTarget, event.target, true, false)) {
        event.preventDefault();
      }
    },
    [selectCommentLine],
  );

  const onShowNewCommentInput = useCallback(
    (event: React.MouseEvent<HTMLTableElement>) => {
      if (suppressNextClick.current) {
        suppressNextClick.current = false;
        return;
      }
      selectCommentLine(event.currentTarget, event.target, event.shiftKey, true);
    },
    [selectCommentLine],
  );

  const onResetNewCommentInput = useCallback(() => {
    setCellAtom(null);
    setCommentRange(null);
    dragSelectionActive.current = false;
  }, [setCellAtom, setCommentRange]);

  const newCommentInputCallbacks: NewCommentInputCallbacks | null = useMemo(() => {
    if (pullRequest != null) {
      return {
        onExtendNewCommentRange,
        onResetNewCommentInput,
        onShowNewCommentInput,
        onStartNewCommentRange,
      };
    }
    return null;
  }, [
    pullRequest,
    onExtendNewCommentRange,
    onResetNewCommentInput,
    onShowNewCommentInput,
    onStartNewCommentRange,
  ]);

  // Diff and tokenize atom
  const diffAndTokenizeParams = useMemo(
    () => ({path, before, after, scopeName, colorMode}),
    [path, before, after, scopeName, colorMode],
  );
  const diffAndTokenizeLoadableAtom = useMemo(
    () => loadable(diffAndTokenizeAtom(diffAndTokenizeParams)),
    [diffAndTokenizeParams],
  );
  const diffAndTokenizeLoadable = useAtomValue(diffAndTokenizeLoadableAtom);

  // Computed lineToPosition for this file path
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

  // Rendering the code only depends on the blob diff and tokenization. Commit
  // IDs, versions, and line-to-position mappings support commenting and can
  // finish in the background. Waiting for them here made every visible diff
  // disappear during a comment-only PR refresh.
  if (diffAndTokenizeLoadable.state === 'loading') {
    return {state: 'loading'};
  }

  if (diffAndTokenizeLoadable.state === 'hasError') {
    return {state: 'hasError', error: diffAndTokenizeLoadable.error as Error};
  }

  const diffAndTokenizeResult = diffAndTokenizeLoadable.data;
  const commitIDs = commitIDsLoadable.state === 'hasData' ? commitIDsLoadable.data : null;

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
