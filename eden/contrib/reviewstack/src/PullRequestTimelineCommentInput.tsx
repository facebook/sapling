/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import PullRequestCommentInput from './PullRequestCommentInput';
import PullRequestReviewSelector from './PullRequestReviewSelector';
import {PullRequestReviewEvent} from './generated/graphql';
import {gitHubClientAtom, gitHubPullRequestAtom, gitHubPullRequestPendingReviewIDAtom} from './jotai';
import useRefreshPullRequest from './useRefreshPullRequest';
import {useAtomValue} from 'jotai';
import {useCallback, useState} from 'react';

export default function PullRequestTimelineCommentInput(): React.ReactElement {
  const pendingReviewID = useAtomValue(gitHubPullRequestPendingReviewIDAtom);
  const refreshPullRequest = useRefreshPullRequest();
  const pullRequest = useAtomValue(gitHubPullRequestAtom);
  const [event, setEvent] = useState(PullRequestReviewEvent.Comment);

  // Client is already loaded by the time we're adding a comment
  const client = useAtomValue(gitHubClientAtom);

  const addComment = useCallback(
    async (comment: string) => {
      if (client == null) {
        return Promise.reject('client not found');
      }

      if (pullRequest == null) {
        return Promise.reject('pull request not found');
      }

      if (pendingReviewID == null) {
        if (event === PullRequestReviewEvent.Comment) {
          await client.addComment(pullRequest.id, comment);
        } else {
          await client.addPullRequestReview({
            body: comment,
            pullRequestId: pullRequest.id,
            event,
          });
        }
      } else {
        await client.submitPullRequestReview({
          body: comment,
          pullRequestId: pullRequest.id,
          pullRequestReviewId: pendingReviewID,
          event,
        });
      }

      refreshPullRequest();
      setEvent(PullRequestReviewEvent.Comment);
    },
    [client, event, pendingReviewID, pullRequest, refreshPullRequest],
  );

  return (
    <PullRequestCommentInput
      addComment={addComment}
      autoFocus={false}
      resetInputAfterAddingComment={true}
      allowEmptyMessage={pendingReviewID != null || event === PullRequestReviewEvent.Approve}
      label="Submit"
      actionSelector={<PullRequestReviewSelector event={event} onSelect={setEvent} />}
    />
  );
}
