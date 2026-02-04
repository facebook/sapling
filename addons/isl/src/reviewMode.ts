/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {atom} from 'jotai';
import {ComparisonType} from 'shared/Comparison';
import {showComparison, dismissComparison} from './ComparisonView/atoms';
import {writeAtom} from './jotaiUtils';

/**
 * State for tracking review mode.
 * When active, the user is reviewing a specific PR's changes.
 */
export type ReviewModeState = {
  active: boolean;
  prNumber: string | null; // DiffId (PR number as string)
  prHeadHash: string | null; // Track PR version for reset detection
};

/**
 * Atom tracking whether we're in review mode and which PR is being reviewed.
 */
export const reviewModeAtom = atom<ReviewModeState>({
  active: false,
  prNumber: null,
  prHeadHash: null,
});

/**
 * Enter review mode for a specific PR.
 * Opens the comparison view for the PR's head commit.
 */
export function enterReviewMode(prNumber: string, headHash: string): void {
  writeAtom(reviewModeAtom, {
    active: true,
    prNumber,
    prHeadHash: headHash,
  });
  showComparison({type: ComparisonType.Committed, hash: headHash});
}

/**
 * Exit review mode and close the comparison view.
 */
export function exitReviewMode(): void {
  writeAtom(reviewModeAtom, {
    active: false,
    prNumber: null,
    prHeadHash: null,
  });
  dismissComparison();
}

/**
 * Navigate to a specific PR within review mode.
 * Used for stack navigation and auto-advance after merge.
 */
export function navigateToPRInStack(prNumber: string, headHash: string): void {
  writeAtom(reviewModeAtom, prev => ({
    ...prev,
    prNumber,
    prHeadHash: headHash,
  }));
  showComparison({type: ComparisonType.Committed, hash: headHash});
}
