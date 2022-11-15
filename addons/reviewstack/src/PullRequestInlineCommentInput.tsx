/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {GitObjectID, ID} from './github/types';

import PullRequestCommentInput from './PullRequestCommentInput';
import {gitHubClient, gitHubPullRequest} from './recoil';
import useRefreshPullRequest from './useRefreshPullRequest';
import {useRecoilCallback} from 'recoil';

type Props = {
  commentID: ID;
  commitID: GitObjectID;
  onCancel: () => void;
};

export default function PullRequestInlineCommentInput({
  commentID,
  commitID,
  onCancel,
}: Props): React.ReactElement {
  const refreshPullRequest = useRefreshPullRequest();
  const addComment = useRecoilCallback<[string], Promise<void>>(
    ({snapshot}) =>
      async comment => {
        const clientLoadable = snapshot.getLoadable(gitHubClient);
        if (clientLoadable.state !== 'hasValue' || clientLoadable.contents == null) {
          return Promise.reject('client not found');
        }
        const client = clientLoadable.contents;

        const pullRequestId = snapshot.getLoadable(gitHubPullRequest).valueMaybe()?.id;
        if (pullRequestId == null) {
          return Promise.reject('pull request not found');
        }

        await client.addPullRequestReviewComment({
          body: comment,
          commitOID: commitID,
          inReplyTo: commentID,
          pullRequestId,
        });

        refreshPullRequest();
      },
    [commentID, commitID, refreshPullRequest],
  );

  return (
    <PullRequestCommentInput
      addComment={addComment}
      onCancel={onCancel}
      autoFocus={true}
      resetInputAfterAddingComment={true}
    />
  );
}
