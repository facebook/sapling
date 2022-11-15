/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import PullRequestCommentInput from './PullRequestCommentInput';
import {DiffSide} from './generated/graphql';
import {
  gitHubClient,
  gitHubPullRequest,
  gitHubPullRequestComparableVersions,
  gitHubPullRequestNewCommentInputCell,
  gitHubPullRequestPositionForLine,
} from './recoil';
import useRefreshPullRequest from './useRefreshPullRequest';
import {Box, Text} from '@primer/react';
import {useRecoilCallback, useResetRecoilState} from 'recoil';

type Props = {
  line: number;
  path: string;
  side: DiffSide;
};

export default function PullRequestNewCommentInput({line, path, side}: Props): React.ReactElement {
  const onCancel = useResetRecoilState(gitHubPullRequestNewCommentInputCell);
  const refreshPullRequest = useRefreshPullRequest();
  const addComment = useRecoilCallback<[string], Promise<void>>(
    ({snapshot}) =>
      async comment => {
        const client = snapshot.getLoadable(gitHubClient).valueMaybe();
        if (client == null) {
          return Promise.reject('client not found');
        }

        const pullRequestId = snapshot.getLoadable(gitHubPullRequest).valueMaybe()?.id;
        if (pullRequestId == null) {
          return Promise.reject('pull request id not found');
        }

        const comparableVersions = snapshot
          .getLoadable(gitHubPullRequestComparableVersions)
          .valueMaybe();
        if (comparableVersions == null) {
          return Promise.reject('comparableVersions not found');
        }

        const {beforeCommitID, afterCommitID} = comparableVersions;
        const commitID =
          beforeCommitID != null && side === DiffSide.Left ? beforeCommitID : afterCommitID;

        const position = snapshot
          .getLoadable(gitHubPullRequestPositionForLine({line, path, side}))
          .valueMaybe();
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

        // Note that onCancel() will reset gitHubPullRequestNewCommentInputCell
        // to null, which will result in this component being removed from the
        // DOM.
        onCancel();
        refreshPullRequest();
      },
    [line, onCancel, path, refreshPullRequest, side],
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
