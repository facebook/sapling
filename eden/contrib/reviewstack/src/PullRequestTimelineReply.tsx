/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ID} from './github/types';

import PullRequestCommentInput from './PullRequestCommentInput';
import {gitHubClientAtom, gitHubPullRequestAtom} from './jotai';
import useRefreshPullRequest from './useRefreshPullRequest';
import {ReplyIcon} from '@primer/octicons-react';
import {Box, IconButton} from '@primer/react';
import {atom, useAtom, useAtomValue} from 'jotai';
import {useCallback} from 'react';

export const pullRequestTimelineReplyIDAtom = atom<ID | null>(null);

export default function PullRequestTimelineReply({
  commentID,
  authorLogin,
}: {
  commentID: ID;
  authorLogin?: string;
}): React.ReactElement {
  const [replyingToID, setReplyingToID] = useAtom(pullRequestTimelineReplyIDAtom);
  const client = useAtomValue(gitHubClientAtom);
  const pullRequest = useAtomValue(gitHubPullRequestAtom);
  const refreshPullRequest = useRefreshPullRequest();
  const initialComment = authorLogin == null ? '' : `@${authorLogin} `;

  const addReply = useCallback(
    async (comment: string) => {
      if (client == null) {
        return Promise.reject('client not found');
      }
      if (pullRequest == null) {
        return Promise.reject('pull request not found');
      }
      await client.addComment(pullRequest.id, comment);
      setReplyingToID(null);
      refreshPullRequest();
    },
    [client, pullRequest, refreshPullRequest, setReplyingToID],
  );

  if (replyingToID === commentID) {
    return (
      <PullRequestCommentInput
        addComment={addReply}
        autoFocus={true}
        initialComment={initialComment}
        label="Reply"
        onCancel={() => setReplyingToID(null)}
        resetInputAfterAddingComment={true}
      />
    );
  }

  return (
    <Box display="flex" justifyContent="flex-end" marginTop={1}>
      <IconButton
        aria-label="Reply to comment"
        icon={ReplyIcon}
        onClick={() => setReplyingToID(commentID)}
        size="small"
        variant="invisible"
      />
    </Box>
  );
}
