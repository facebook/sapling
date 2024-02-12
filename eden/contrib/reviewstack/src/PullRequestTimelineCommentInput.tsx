/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import PullRequestCommentInput from './PullRequestCommentInput';
import PullRequestReviewSelector from './PullRequestReviewSelector';
import {PullRequestReviewEvent} from './generated/graphql';
import {gitHubClient, gitHubPullRequest, gitHubPullRequestPendingReviewID} from './recoil';
import useRefreshPullRequest from './useRefreshPullRequest';
import {useState} from 'react';
import {useRecoilCallback, useRecoilValue} from 'recoil';

export default function PullRequestTimelineCommentInput(): React.ReactElement {
  const pendingReviewID = useRecoilValue(gitHubPullRequestPendingReviewID);
  const refreshPullRequest = useRefreshPullRequest();
  const [event, setEvent] = useState(PullRequestReviewEvent.Comment);
  const addComment = useRecoilCallback<[string], Promise<void>>(
    ({snapshot}) =>
      async comment => {
        const clientLoadable = snapshot.getLoadable(gitHubClient);
        if (clientLoadable.state !== 'hasValue' || clientLoadable.contents == null) {
          return Promise.reject('client not found');
        }
        const client = clientLoadable.contents;

        const pullRequestLoadable = snapshot.getLoadable(gitHubPullRequest);
        if (pullRequestLoadable.state !== 'hasValue' || pullRequestLoadable.contents == null) {
          return Promise.reject('pull request not found');
        }
        const pullRequest = pullRequestLoadable.contents;

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
    [event, pendingReviewID, refreshPullRequest],
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
