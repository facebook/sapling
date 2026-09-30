/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {StackPullRequestFragment} from './generated/graphql';
import type GitHubClient from './github/GitHubClient';

import effectivePullRequestReviewDecision from './effectivePullRequestReviewDecision';
import {
  MergeableState,
  PullRequestMergeMethod,
  PullRequestReviewDecision,
  PullRequestState,
} from './generated/graphql';

export type PullRequestMergeCandidate = Pick<
  StackPullRequestFragment,
  | 'baseRefName'
  | 'headRefOid'
  | 'id'
  | 'isDraft'
  | 'latestReviews'
  | 'mergeable'
  | 'number'
  | 'reviewDecision'
  | 'state'
  | 'title'
  | 'viewerCanUpdate'
>;

export type PullRequestMergePlan = {
  blockers: string[];
  pullRequests: PullRequestMergeCandidate[];
  totalPullRequests: number;
};

/**
 * Sapling writes stack entries from newest to oldest. Landing a selected
 * change therefore means taking the suffix that starts at that change and
 * reversing it so GitHub receives merge requests from the root upward.
 */
export function buildPullRequestMergePlan(
  stack: PullRequestMergeCandidate[],
  selectedPullRequest: number,
): PullRequestMergePlan {
  const selectedIndex = stack.findIndex(({number}) => number === selectedPullRequest);
  if (selectedIndex === -1) {
    return {
      blockers: ['The selected pull request is missing from its Sapling stack.'],
      pullRequests: [],
      totalPullRequests: 0,
    };
  }

  const blockers: string[] = [];
  const pullRequests: PullRequestMergeCandidate[] = [];
  let totalPullRequests = 0;
  stack
    .slice(selectedIndex)
    .reverse()
    .forEach(pullRequest => {
      const label = `#${pullRequest.number}`;
      if (pullRequest.state === PullRequestState.Merged) {
        return;
      }
      if (pullRequest.state !== PullRequestState.Open) {
        blockers.push(`${label} is closed without being merged.`);
        return;
      }
      totalPullRequests++;
      if (!pullRequest.viewerCanUpdate) {
        blockers.push(`You do not have permission to merge ${label}.`);
        return;
      }
      if (pullRequest.baseRefName !== 'main') {
        blockers.push(`${label} targets ${pullRequest.baseRefName}, not main.`);
        return;
      }
      if (pullRequest.isDraft) {
        blockers.push(`${label} is still a draft.`);
        return;
      }
      const reviewDecision = effectivePullRequestReviewDecision(
        pullRequest.reviewDecision,
        pullRequest.latestReviews?.nodes ?? [],
      );
      if (reviewDecision !== PullRequestReviewDecision.Approved) {
        blockers.push(`${label} is not approved.`);
        return;
      }
      if (pullRequest.mergeable === MergeableState.Conflicting) {
        blockers.push(`${label} has merge conflicts.`);
        return;
      }
      pullRequests.push(pullRequest);
    });

  return {blockers, pullRequests, totalPullRequests};
}

export async function mergePullRequestStack(
  client: GitHubClient,
  pullRequests: PullRequestMergeCandidate[],
  onProgress?: (completed: number, total: number) => void,
): Promise<void> {
  for (let index = 0; index < pullRequests.length; index++) {
    const pullRequest = pullRequests[index];
    onProgress?.(index, pullRequests.length);
    // Stack landing must be ordered because every successful merge changes main.
    // eslint-disable-next-line no-await-in-loop
    const result = await client.mergePullRequest({
      expectedHeadOid: pullRequest.headRefOid,
      mergeMethod: PullRequestMergeMethod.Squash,
      pullRequestId: pullRequest.id,
    });
    if (result.mergePullRequest?.pullRequest?.merged !== true) {
      throw new Error(`GitHub did not merge #${pullRequest.number}.`);
    }
    onProgress?.(index + 1, pullRequests.length);
  }
}
