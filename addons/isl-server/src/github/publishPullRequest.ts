/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import queryGraphQL from './queryGraphQL';

const MARK_READY_FOR_REVIEW_MUTATION = `
mutation MarkPullRequestReadyForReview($pullRequestId: ID!) {
  markPullRequestReadyForReview(input: {
    pullRequestId: $pullRequestId
  }) {
    pullRequest {
      id
      isDraft
    }
  }
}
`;

type MarkReadyForReviewResponse = {
  markPullRequestReadyForReview: {
    pullRequest: {
      id: string;
      isDraft: boolean;
    };
  };
};

type MarkReadyForReviewVariables = {
  pullRequestId: string;
};

/**
 * Mark a draft PR as ready for review using GitHub's markPullRequestReadyForReview mutation.
 */
export async function publishPullRequest(
  hostname: string,
  pullRequestId: string,
): Promise<string> {
  const variables: MarkReadyForReviewVariables = {pullRequestId};

  const response = await queryGraphQL<MarkReadyForReviewResponse, MarkReadyForReviewVariables>(
    MARK_READY_FOR_REVIEW_MUTATION,
    variables,
    hostname,
  );

  return response.markPullRequestReadyForReview.pullRequest.id;
}
