/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffSignalSummary, MergeableState, MergeStateStatus} from '../types';
import type {PullRequestReviewDecision} from 'isl-server/src/github/generated/graphql';

import {atom} from 'jotai';

/**
 * Tracks which PR is currently being merged (if any).
 * Used to show loading state and prevent double-merge attempts.
 */
export const mergeInProgressAtom = atom<string | null>(null);

/**
 * Result of mergeability derivation.
 */
export type MergeabilityStatus = {
  canMerge: boolean;
  reasons: string[];
};

/**
 * PR data needed for mergeability check.
 */
export type PRMergeabilityData = {
  signalSummary?: DiffSignalSummary;
  reviewDecision?: PullRequestReviewDecision;
  mergeable?: MergeableState;
  mergeStateStatus?: MergeStateStatus;
  state?: string;
};

/**
 * Derive whether a PR can be merged based on CI status, reviews, and merge state.
 * Implements the logic for MRG-03 (merge button disabled when CI failing or reviews pending).
 * Returns reasons array so UI can show why merge is disabled.
 */
export function deriveMergeability(pr: PRMergeabilityData): MergeabilityStatus {
  const reasons: string[] = [];

  // Check if PR is already merged or closed
  if (pr.state === 'MERGED') {
    return {canMerge: false, reasons: ['PR is already merged']};
  }
  if (pr.state === 'CLOSED') {
    return {canMerge: false, reasons: ['PR is closed']};
  }

  // Check CI status (MRG-03)
  if (pr.signalSummary === 'failed') {
    reasons.push('CI checks are failing');
  } else if (pr.signalSummary === 'running') {
    reasons.push('CI checks are still running');
  }

  // Check review decision (MRG-03)
  if (pr.reviewDecision === 'CHANGES_REQUESTED') {
    reasons.push('Changes have been requested');
  } else if (pr.reviewDecision === 'REVIEW_REQUIRED') {
    reasons.push('Review approval is required');
  }

  // Check merge conflicts
  if (pr.mergeable === 'CONFLICTING') {
    reasons.push('Merge conflicts exist');
  }

  // Check detailed merge state status
  if (pr.mergeStateStatus === 'BLOCKED') {
    reasons.push('Blocked by branch protection rules');
  } else if (pr.mergeStateStatus === 'BEHIND') {
    reasons.push('Branch is behind base branch');
  } else if (pr.mergeStateStatus === 'DRAFT') {
    reasons.push('PR is a draft');
  } else if (pr.mergeStateStatus === 'UNSTABLE') {
    // This is often redundant with CI check, but include if not already
    if (!reasons.some(r => r.includes('CI'))) {
      reasons.push('Required checks are not passing');
    }
  }

  return {
    canMerge: reasons.length === 0,
    reasons,
  };
}

/**
 * Format merge reasons for display.
 */
export function formatMergeBlockReasons(reasons: string[]): string {
  if (reasons.length === 0) {
    return 'Ready to merge';
  }
  if (reasons.length === 1) {
    return reasons[0];
  }
  return `${reasons[0]} (+${reasons.length - 1} more)`;
}
