/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {GitObjectID, ID} from './github/types';

import PullRequestCommentInput from './PullRequestCommentInput';
import {gitHubClientAtom, gitHubPullRequestAtom} from './jotai';
import useRefreshPullRequest from './useRefreshPullRequest';
import {useAtomValue} from 'jotai';
import {loadable} from 'jotai/utils';
import {useCallback, useMemo} from 'react';

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
  const pullRequest = useAtomValue(gitHubPullRequestAtom);

  // Load the GitHub client asynchronously
  const loadableClient = useMemo(() => loadable(gitHubClientAtom), []);
  const clientLoadable = useAtomValue(loadableClient);
  const client = clientLoadable.state === 'hasData' ? clientLoadable.data : null;

  const addComment = useCallback(
    async (comment: string) => {
      if (client == null) {
        return Promise.reject('client not found');
      }

      const pullRequestId = pullRequest?.id;
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
    [client, commentID, commitID, pullRequest, refreshPullRequest],
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
