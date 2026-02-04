/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DraftPullRequestReviewThread, PullRequestReviewEvent} from 'isl/src/types';
import queryGraphQL from './queryGraphQL';

const ADD_PULL_REQUEST_REVIEW_MUTATION = `
mutation AddPullRequestReview(
  $pullRequestId: ID!
  $event: PullRequestReviewEvent!
  $body: String
  $threads: [DraftPullRequestReviewThread!]
) {
  addPullRequestReview(input: {
    pullRequestId: $pullRequestId
    event: $event
    body: $body
    threads: $threads
  }) {
    pullRequestReview {
      id
    }
  }
}
`;

type AddPullRequestReviewResponse = {
  addPullRequestReview: {
    pullRequestReview: {
      id: string;
    };
  };
};

type AddPullRequestReviewVariables = {
  pullRequestId: string;
  event: PullRequestReviewEvent;
  body?: string;
  threads?: DraftPullRequestReviewThread[];
};

/**
 * Submit a PR review with approval decision and optional comments.
 * Uses GitHub's addPullRequestReview mutation which creates and submits in one operation.
 */
export async function submitPullRequestReview(
  hostname: string,
  pullRequestId: string,
  event: PullRequestReviewEvent,
  body?: string,
  threads?: DraftPullRequestReviewThread[],
): Promise<string> {
  const variables: AddPullRequestReviewVariables = {
    pullRequestId,
    event,
    body: body || undefined,
    threads: threads && threads.length > 0 ? threads : undefined,
  };

  const response = await queryGraphQL<AddPullRequestReviewResponse, AddPullRequestReviewVariables>(
    ADD_PULL_REQUEST_REVIEW_MUTATION,
    variables,
    hostname,
  );

  return response.addPullRequestReview.pullRequestReview.id;
}
