/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import effectivePullRequestReviewDecision from './effectivePullRequestReviewDecision';
import {
  PullRequestReviewDecision,
  PullRequestReviewState,
} from './generated/graphql';

test('preserves an explicit GitHub review decision', () => {
  expect(
    effectivePullRequestReviewDecision(PullRequestReviewDecision.ReviewRequired, [
      {state: PullRequestReviewState.Approved},
    ]),
  ).toBe(PullRequestReviewDecision.ReviewRequired);
});

test('uses a latest approval when GitHub omits the aggregate decision', () => {
  expect(
    effectivePullRequestReviewDecision(null, [
      {state: PullRequestReviewState.Commented},
      {state: PullRequestReviewState.Approved},
    ]),
  ).toBe(PullRequestReviewDecision.Approved);
});

test('gives changes requested precedence over approval', () => {
  expect(
    effectivePullRequestReviewDecision(null, [
      {state: PullRequestReviewState.Approved},
      {state: PullRequestReviewState.ChangesRequested},
    ]),
  ).toBe(PullRequestReviewDecision.ChangesRequested);
});

test('keeps a missing decision when no decisive review exists', () => {
  expect(
    effectivePullRequestReviewDecision(undefined, [
      null,
      {state: PullRequestReviewState.Commented},
    ]),
  ).toBeNull();
});
