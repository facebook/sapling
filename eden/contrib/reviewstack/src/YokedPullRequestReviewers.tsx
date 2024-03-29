/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {UserFragment} from './generated/graphql';

import FieldLabel from './FieldLabel';
import YokedRepoAssignableUsersInput from './YokedRepoAssignableUsersInput';
import {gitHubUsername} from './github/gitHubCredentials';
import {
  gitHubClient,
  gitHubPullRequest,
  gitHubPullRequestReviewers,
  gitHubPullRequestViewerDidAuthor,
} from './recoil';
import useRefreshPullRequest from './useRefreshPullRequest';
import {GearIcon, PeopleIcon} from '@primer/octicons-react';
import {ActionMenu, AvatarToken, Box, Button, StyledOcticon, IssueLabelToken} from '@primer/react';
import {useEffect} from 'react';
import {useRecoilCallback, useRecoilState, useRecoilValue} from 'recoil';

export default function PullRequestReviewers(): React.ReactElement {
  const refreshPullRequest = useRefreshPullRequest();
  const pullRequest = useRecoilValue(gitHubPullRequest);
  const [pullRequestReviewers, setPullRequestReviewers] = useRecoilState(
    gitHubPullRequestReviewers,
  );
  const viewerDidAuthor = useRecoilValue(gitHubPullRequestViewerDidAuthor);
  const username = useRecoilValue(gitHubUsername);

  // Initialize pullRequestReviewers state using pullRequest once it is available.
  useEffect(() => {
    if (pullRequest != null) {
      // From https://docs.github.com/en/rest/pulls/review-requests:
      //
      // > Gets the users or teams whose review is requested for a pull request.
      // > Once a requested reviewer submits a review, they are no longer
      // > considered a requested reviewer.
      //
      // As such, both the `reviews` and `reviewRequests` fields must be
      // consulted to get the full list of reviewers.
      const reviewers: Array<UserFragment> = [];
      const reviewerIDs: Set<string> = new Set();
      for (const user of pullRequest.reviews?.nodes ?? []) {
        const author = user?.author;
        if (author?.__typename === 'User' && author.login !== username) {
          if (!reviewerIDs.has(author.id)) {
            reviewerIDs.add(author.id);
            reviewers.push(author);
          }
        }
      }
      for (const node of pullRequest.reviewRequests?.nodes ?? []) {
        const reviewer = node?.requestedReviewer;
        if (reviewer?.__typename === 'User') {
          if (!reviewerIDs.has(reviewer.id)) {
            reviewerIDs.add(reviewer.id);
            reviewers.push(reviewer);
          }
        }
      }

      setPullRequestReviewers({reviewers, reviewerIDs});
    }
  }, [pullRequest, setPullRequestReviewers, username]);

  const updateReviewers = useRecoilCallback<[UserFragment, boolean], Promise<void>>(
    ({snapshot}) =>
      async (user: UserFragment, isExisting: boolean) => {
        const client = snapshot.getLoadable(gitHubClient).valueMaybe();
        if (client == null) {
          return Promise.reject('client not found');
        }

        const pullRequestId = snapshot.getLoadable(gitHubPullRequest).valueMaybe()?.id;
        if (pullRequestId == null) {
          return Promise.reject('pull request not found');
        }

        try {
          // When adding or removing a reviewer, optimistically update
          // pullRequestReviewers and the UI instead of waiting for the respective
          // API call to return.
          const reviewerIDs = new Set(pullRequestReviewers.reviewerIDs);
          let reviewers: UserFragment[];
          if (!isExisting) {
            reviewers = pullRequestReviewers.reviewers.concat(user);
            reviewers.sort((a, b) => a.login.localeCompare(b.login));
            reviewerIDs.add(user.id);
          } else {
            reviewers = pullRequestReviewers.reviewers.filter(({id}) => user.id !== id);
            reviewerIDs.delete(user.id);
          }
          setPullRequestReviewers({reviewers, reviewerIDs});
          await client.requestReviews({
            pullRequestId,
            userIds: [...reviewerIDs],
          });
          refreshPullRequest();
        } catch {
          // If there is an error, roll back the update by resetting
          // pullRequestReviewers to its previous value.
          setPullRequestReviewers(pullRequestReviewers);
        }
      },
    [pullRequestReviewers, refreshPullRequest, setPullRequestReviewers],
  );

  const label = !viewerDidAuthor ? null : (
    <ActionMenu>
      <ActionMenu.Anchor>
        <button className="pr-label-button">
          <StyledOcticon icon={PeopleIcon} size={'small'} />
        </button>
      </ActionMenu.Anchor>
      <ActionMenu.Overlay width="medium">
        <YokedRepoAssignableUsersInput
          existingUserIDs={pullRequestReviewers.reviewerIDs}
          onSelect={updateReviewers}
        />
      </ActionMenu.Overlay>
    </ActionMenu>
  );

  return (
    <Box display="flex" alignItems="center" gridGap={2} paddingLeft={3}>
      {label}
      <Box display="flex" flexWrap="wrap" gridGap={1}>
        {pullRequestReviewers.reviewers.map(user => (
          <IssueLabelToken
            style={{
              color: '#57606a',
              background: 'none',
              borderColor: 'rgba(27,31,36,0.15)',
            }}
            key={user.id}
            text={user.login}
            fillColor={`rgba(234,238,242,0.5)`}
            size="large"
            onRemove={!viewerDidAuthor ? undefined : () => updateReviewers(user, true)}
            hideRemoveButton={!viewerDidAuthor}
          />
        ))}
      </Box>
    </Box>
  );

  return (
    <Box display="flex" alignItems="center" gridGap={2} paddingLeft={3}>
      {label}
      <Box display="flex" flexWrap="wrap" gridGap={1}>
        {pullRequestReviewers.reviewers.map(user => (
          <AvatarToken
            key={user.id}
            avatarSrc={user.avatarUrl}
            text={user.login}
            size="large"
            onRemove={!viewerDidAuthor ? undefined : () => updateReviewers(user, true)}
            style={{background: 'none'}}
          />
        ))}
      </Box>
    </Box>
  );
}
