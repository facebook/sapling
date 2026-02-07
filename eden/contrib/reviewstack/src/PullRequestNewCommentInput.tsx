/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import PullRequestCommentInput from './PullRequestCommentInput';
import {DiffSide} from './generated/graphql';
import {
  gitHubClientAtom,
  gitHubPullRequestAtom,
  gitHubPullRequestComparableVersionsAtom,
  gitHubPullRequestNewCommentInputCellAtom,
  gitHubPullRequestPositionForLineAtom,
} from './jotai';
import useRefreshPullRequest from './useRefreshPullRequest';
import {Box, Text} from '@primer/react';
import {useAtomValue, useSetAtom} from 'jotai';
import {useMemo, useCallback} from 'react';

type Props = {
  line: number;
  path: string;
  side: DiffSide;
};

export default function PullRequestNewCommentInput({line, path, side}: Props): React.ReactElement {
  const setCellAtom = useSetAtom(gitHubPullRequestNewCommentInputCellAtom);
  const onCancel = useCallback(() => setCellAtom(null), [setCellAtom]);
  const refreshPullRequest = useRefreshPullRequest();

  // Client is already loaded by the time we're adding a comment
  const client = useAtomValue(gitHubClientAtom);

  // Read pull request and comparable versions from Jotai
  const pullRequest = useAtomValue(gitHubPullRequestAtom);
  const comparableVersions = useAtomValue(gitHubPullRequestComparableVersionsAtom);

  // Get position for this line using the Jotai atom
  const positionAtom = useMemo(
    () => gitHubPullRequestPositionForLineAtom({line, path, side}),
    [line, path, side],
  );
  const position = useAtomValue(positionAtom);

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

      const {beforeCommitID, afterCommitID} = comparableVersions;
      const commitID =
        beforeCommitID != null && side === DiffSide.Left ? beforeCommitID : afterCommitID;

      if (position == null) {
        return Promise.reject('positionForLine not found');
      }

      await client.addPullRequestReviewComment({
        body: comment,
        commitOID: commitID,
        path,
        position,
        pullRequestId,
      });

      // Note that onCancel() will reset gitHubPullRequestNewCommentInputCellAtom
      // to null, which will result in this component being removed from the
      // DOM.
      onCancel();
      refreshPullRequest();
    },
    [client, comparableVersions, onCancel, path, position, pullRequest, refreshPullRequest, side],
  );

  return (
    <Box backgroundColor="canvas.subtle" fontFamily="normal" padding={2}>
      <Box borderColor="border.default" borderWidth={1} borderStyle="solid">
        <Box padding={2}>
          <Text>
            Commenting on <Text fontWeight="bold">line {line}</Text>
          </Text>
        </Box>
        {/* Do not reset input after adding a comment because addComment unmounts it. */}
        <PullRequestCommentInput
          addComment={addComment}
          onCancel={onCancel}
          autoFocus={true}
          resetInputAfterAddingComment={false}
        />
      </Box>
    </Box>
  );
}
