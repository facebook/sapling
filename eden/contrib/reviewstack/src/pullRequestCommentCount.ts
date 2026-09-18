/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {StackPullRequestFragment} from './generated/graphql';

export function countPullRequestComments(
  fragment: Pick<StackPullRequestFragment, 'comments' | 'reviews' | 'reviewThreads'>,
): number {
  const reviewBodyCount = (fragment.reviews?.nodes ?? []).filter(
    review => (review?.body.trim().length ?? 0) > 0,
  ).length;
  const reviewThreadCommentCount = (fragment.reviewThreads?.nodes ?? []).reduce(
    (count, thread) => count + (thread?.comments.totalCount ?? 0),
    0,
  );
  return fragment.comments.totalCount + reviewBodyCount + reviewThreadCommentCount;
}
