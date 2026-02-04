/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {randomId} from 'shared/utils';
import {localStorageBackedAtomFamily, readAtom, writeAtom} from '../jotaiUtils';

/**
 * A pending comment that has not yet been submitted to GitHub.
 * Pending comments are managed client-side and batched for submission
 * because GitHub's API doesn't support incremental pending reviews.
 */
export type PendingComment = {
  /** Unique client-side ID */
  id: string;
  /** Comment type: inline (on line), file (on file), or pr (review body) */
  type: 'inline' | 'file' | 'pr';
  /** Comment body text */
  body: string;
  /** File path for inline/file comments */
  path?: string;
  /** Line number for inline comments */
  line?: number;
  /** Which side of the diff (LEFT = old, RIGHT = new) */
  side?: 'LEFT' | 'RIGHT';
  /** Timestamp when the comment was created */
  createdAt: number;
};

/**
 * Per-PR pending comments state with localStorage persistence.
 * Key is PR number as string.
 * Comments expire after 7 days to prevent stale data accumulation.
 */
export const pendingCommentsAtom = localStorageBackedAtomFamily<string, PendingComment[]>(
  'isl.pending-comments:',
  () => [],
  7, // Expire after 7 days
);

/**
 * Add a pending comment for a PR.
 * Generates a unique ID and timestamp automatically.
 */
export function addPendingComment(
  prNumber: string,
  comment: Omit<PendingComment, 'id' | 'createdAt'>,
): void {
  const atom = pendingCommentsAtom(prNumber);
  const newComment: PendingComment = {
    ...comment,
    id: randomId(),
    createdAt: Date.now(),
  };
  writeAtom(atom, prev => [...prev, newComment]);
}

/**
 * Remove a pending comment by ID from a PR.
 */
export function removePendingComment(prNumber: string, commentId: string): void {
  const atom = pendingCommentsAtom(prNumber);
  writeAtom(atom, prev => prev.filter(c => c.id !== commentId));
}

/**
 * Clear all pending comments for a PR.
 * Call this after successfully submitting a review.
 */
export function clearPendingComments(prNumber: string): void {
  const atom = pendingCommentsAtom(prNumber);
  writeAtom(atom, []);
}

/**
 * Get the count of pending comments for a PR.
 * Useful for badge display in the UI.
 */
export function getPendingCommentCount(prNumber: string): number {
  const atom = pendingCommentsAtom(prNumber);
  return readAtom(atom).length;
}
