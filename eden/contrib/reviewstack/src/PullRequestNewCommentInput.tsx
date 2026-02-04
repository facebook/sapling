/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import PullRequestCommentInput from './PullRequestCommentInput';
import {DiffSide} from './generated/graphql';
import {gitHubClientAtom, gitHubPullRequestAtom} from './jotai';
import type {ComparableVersions} from './jotai';
import {gitHubPullRequestComparableVersionsAtom} from './jotai';
import {gitHubPullRequestNewCommentInputCell, gitHubPullRequestPositionForLine} from './recoil';
import useRefreshPullRequest from './useRefreshPullRequest';
import {Box, Text} from '@primer/react';
import {useAtomValue} from 'jotai';
import {loadable} from 'jotai/utils';
import {useMemo, useCallback} from 'react';
import {useRecoilCallback, useResetRecoilState} from 'recoil';

type Props = {
  line: number;
  path: string;
  side: DiffSide;
};

export default function PullRequestNewCommentInput({line, path, side}: Props): React.ReactElement {
  const onCancel = useResetRecoilState(gitHubPullRequestNewCommentInputCell);
  const refreshPullRequest = useRefreshPullRequest();

  // Use Jotai loadable pattern for async client access in callbacks
  const loadableClient = useMemo(() => loadable(gitHubClientAtom), []);
  const clientLoadable = useAtomValue(loadableClient);
  const client = clientLoadable.state === 'hasData' ? clientLoadable.data : null;

  // Read pull request and comparable versions from Jotai
  const pullRequest = useAtomValue(gitHubPullRequestAtom);
  const comparableVersions = useAtomValue(gitHubPullRequestComparableVersionsAtom);

  const addComment = useRecoilCallback<[string], Promise<void>>(
    ({snapshot}) =>
      async comment => {
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
    [client, comparableVersions, line, onCancel, path, pullRequest, refreshPullRequest, side],
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
