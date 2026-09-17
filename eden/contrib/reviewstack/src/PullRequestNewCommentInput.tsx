/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffSide} from './generated/graphql';

import PullRequestCommentInput from './PullRequestCommentInput';
import {
  gitHubClientAtom,
  gitHubPullRequestAtom,
  gitHubPullRequestComparableVersionsAtom,
  gitHubPullRequestNewCommentInputCellAtom,
  gitHubPullRequestPendingReviewIDAtom,
} from './jotai';
import {reviewCommentRangeAtom} from './reviewCommentRange';
import useRefreshPullRequest from './useRefreshPullRequest';
import {Box, Text} from '@primer/react';
import {useAtomValue, useSetAtom} from 'jotai';
import {useCallback, useMemo} from 'react';

type Props = {
  line: number;
  path: string;
  side: DiffSide;
};

function getSelectedLineText(range: {
  startLine: number;
  endLine: number;
  path: string;
  side: DiffSide;
}): string {
  const contentByLine = new Map<number, string>();
  document.querySelectorAll<HTMLElement>('[data-review-comment-line-content]').forEach(element => {
    if (element.dataset.path !== range.path || element.dataset.side !== range.side) {
      return;
    }
    const lineNumber = Number(element.dataset.lineNumber);
    if (lineNumber >= range.startLine && lineNumber <= range.endLine) {
      contentByLine.set(lineNumber, element.textContent ?? '');
    }
  });
  return Array.from({length: range.endLine - range.startLine + 1}, (_, index) => {
    const lineNumber = range.startLine + index;
    const content = contentByLine.get(lineNumber);
    if (content == null) {
      throw new Error(`Could not read selected diff line ${lineNumber}.`);
    }
    return content;
  }).join('\n');
}

export default function PullRequestNewCommentInput({line, path, side}: Props): React.ReactElement {
  const setCellAtom = useSetAtom(gitHubPullRequestNewCommentInputCellAtom);
  const setRange = useSetAtom(reviewCommentRangeAtom);
  const onCancel = useCallback(() => {
    setCellAtom(null);
    setRange(null);
  }, [setCellAtom, setRange]);
  const refreshPullRequest = useRefreshPullRequest();

  // Client is already loaded by the time we're adding a comment
  const client = useAtomValue(gitHubClientAtom);

  // Read pull request and comparable versions from Jotai
  const pullRequest = useAtomValue(gitHubPullRequestAtom);
  const comparableVersions = useAtomValue(gitHubPullRequestComparableVersionsAtom);
  const pendingReviewID = useAtomValue(gitHubPullRequestPendingReviewIDAtom);

  const selectedRange = useAtomValue(reviewCommentRangeAtom);
  const range = useMemo(
    () =>
      selectedRange != null &&
      selectedRange.path === path &&
      selectedRange.side === side &&
      selectedRange.endLine === line
        ? selectedRange
        : {anchorLine: line, startLine: line, endLine: line, path, side},
    [line, path, selectedRange, side],
  );

  const addComment = useCallback(
    async (comment: string): Promise<void> => {
      if (client == null) {
        return Promise.reject('client not found');
      }

      const pullRequestId = pullRequest?.id;
      if (pullRequestId == null) {
        return Promise.reject('pull request id not found');
      }

      if (comparableVersions == null) {
        return Promise.reject('comparableVersions not found');
      }

      const thread = {
        body: comment,
        line: range.endLine,
        path,
        side,
        ...(range.startLine === range.endLine
          ? {}
          : {startLine: range.startLine, startSide: side}),
      };
      if (pendingReviewID == null) {
        await client.addPullRequestReview({
          commitOID: comparableVersions.afterCommitID,
          pullRequestId,
          threads: [thread],
        });
      } else {
        await client.addPullRequestReviewThread({
          ...thread,
          pullRequestReviewId: pendingReviewID,
        });
      }

      // Note that onCancel() will reset gitHubPullRequestNewCommentInputCellAtom
      // to null, which will result in this component being removed from the
      // DOM.
      onCancel();
      refreshPullRequest();
    },
    [
      client,
      comparableVersions,
      onCancel,
      path,
      pendingReviewID,
      pullRequest,
      range,
      refreshPullRequest,
      side,
    ],
  );

  const lineLabel =
    range.startLine === range.endLine
      ? `line ${range.endLine}`
      : `lines ${range.startLine}–${range.endLine}`;
  const suggestedChangeText = getSelectedLineText(range);

  return (
    <Box backgroundColor="canvas.subtle" fontFamily="normal" padding={2}>
      <Box borderColor="border.default" borderWidth={1} borderStyle="solid">
        <Box padding={2}>
          <Text>
            Commenting on <Text fontWeight="bold">{lineLabel}</Text>
          </Text>
          <Text as="p" color="fg.muted" fontSize={0} marginBottom={0}>
            Shift-click or drag across line numbers to select a contiguous range.
          </Text>
        </Box>
        {/* Do not reset input after adding a comment because addComment unmounts it. */}
        <PullRequestCommentInput
          addComment={addComment}
          onCancel={onCancel}
          autoFocus={true}
          resetInputAfterAddingComment={false}
          enableSuggestedChange={true}
          suggestedChangeText={suggestedChangeText}
        />
      </Box>
    </Box>
  );
}
