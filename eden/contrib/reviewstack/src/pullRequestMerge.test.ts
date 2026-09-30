/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type GitHubClient from './github/GitHubClient';
import type {PullRequestMergeCandidate} from './pullRequestMerge';

import {
  MergeableState,
  PullRequestReviewDecision,
  PullRequestReviewState,
  PullRequestState,
} from './generated/graphql';
import {buildPullRequestMergePlan, mergePullRequestStack} from './pullRequestMerge';

function pullRequest(
  number: number,
  overrides: Partial<PullRequestMergeCandidate> = {},
): PullRequestMergeCandidate {
  return {
    baseRefName: 'main',
    headRefOid: `head-${number}`,
    id: `id-${number}`,
    isDraft: false,
    latestReviews: {nodes: []},
    mergeable: MergeableState.Mergeable,
    number,
    reviewDecision: PullRequestReviewDecision.Approved,
    state: PullRequestState.Open,
    title: `Change ${number}`,
    viewerCanUpdate: true,
    ...overrides,
  };
}

test('lands the selected Sapling prefix from oldest to newest', () => {
  const stack = [6, 5, 4, 3, 2, 1].map(number => pullRequest(number));

  const plan = buildPullRequestMergePlan(stack, 4);

  expect(plan.blockers).toEqual([]);
  expect(plan.totalPullRequests).toBe(4);
  expect(plan.pullRequests.map(({number}) => number)).toEqual([1, 2, 3, 4]);
});

test('skips merged parents and blocks an unsafe stack before landing', () => {
  const plan = buildPullRequestMergePlan(
    [
      pullRequest(3, {mergeable: MergeableState.Conflicting}),
      pullRequest(2, {reviewDecision: PullRequestReviewDecision.ReviewRequired}),
      pullRequest(1, {state: PullRequestState.Merged}),
    ],
    3,
  );

  expect(plan.pullRequests).toEqual([]);
  expect(plan.totalPullRequests).toBe(2);
  expect(plan.blockers).toEqual(['#2 is not approved.', '#3 has merge conflicts.']);
});

test('accepts the latest approval fallback used by ReviewStack', () => {
  const plan = buildPullRequestMergePlan(
    [
      pullRequest(1, {
        latestReviews: {nodes: [{state: PullRequestReviewState.Approved}]},
        reviewDecision: null,
      }),
    ],
    1,
  );

  expect(plan.blockers).toEqual([]);
  expect(plan.pullRequests.map(({number}) => number)).toEqual([1]);
});

test('merges sequentially and stops after the first GitHub failure', async () => {
  const candidates = [1, 2, 3].map(number => pullRequest(number));
  const mergePullRequest = jest
    .fn()
    .mockResolvedValueOnce({mergePullRequest: {pullRequest: {merged: true}}})
    .mockRejectedValueOnce(new Error('required check failed'));
  const client = {mergePullRequest} as unknown as GitHubClient;
  const progress: Array<[number, number]> = [];

  await expect(
    mergePullRequestStack(client, candidates, (completed, total) =>
      progress.push([completed, total]),
    ),
  ).rejects.toThrow('required check failed');

  expect(mergePullRequest).toHaveBeenCalledTimes(2);
  expect(mergePullRequest.mock.calls.map(([input]) => input)).toEqual([
    {expectedHeadOid: 'head-1', mergeMethod: 'SQUASH', pullRequestId: 'id-1'},
    {expectedHeadOid: 'head-2', mergeMethod: 'SQUASH', pullRequestId: 'id-2'},
  ]);
  expect(progress).toEqual([
    [0, 3],
    [1, 3],
    [1, 3],
  ]);
});
