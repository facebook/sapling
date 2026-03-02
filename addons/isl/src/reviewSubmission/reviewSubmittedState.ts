/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {atom} from 'jotai';
import {writeAtom} from '../jotaiUtils';

/**
 * Tracks the most recent review submission for the current review session.
 * Reset when entering review mode. Used by ReviewActionsBar to
 * swap buttons for a status indicator after successful submission.
 */
export type ReviewSubmittedState = {
  event: 'APPROVE' | 'REQUEST_CHANGES' | 'COMMENT';
  timestamp: Date;
} | null;

export const reviewSubmittedAtom = atom<ReviewSubmittedState>(null);

/** Reset submitted state (call when entering review mode). */
export function resetReviewSubmitted(): void {
  writeAtom(reviewSubmittedAtom, null);
}
