/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {
  PullRequestReviewDecision,
  PullRequestReviewState,
} from './generated/graphql';

/**
 * GitHub can return a null reviewDecision for repositories protected by a
 * ruleset even when latestReviews contains a current approval. Preserve an
 * explicit aggregate decision, otherwise derive one from the latest review by
 * each reviewer.
 */
export default function effectivePullRequestReviewDecision(
  reviewDecision: PullRequestReviewDecision | null | undefined,
  latestReviews: ReadonlyArray<{state: PullRequestReviewState} | null | undefined>,
): PullRequestReviewDecision | null {
  if (reviewDecision != null) {
    return reviewDecision;
  }

  const states = latestReviews.map(review => review?.state);
  if (states.includes(PullRequestReviewState.ChangesRequested)) {
    return PullRequestReviewDecision.ChangesRequested;
  }
  if (states.includes(PullRequestReviewState.Approved)) {
    return PullRequestReviewDecision.Approved;
  }
  return null;
}
