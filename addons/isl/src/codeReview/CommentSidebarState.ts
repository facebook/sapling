/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffComment} from '../types';

import {localStorageBackedAtom} from '../jotaiUtils';

/**
 * Whether the comment sidebar is open.
 * Persisted in localStorage so it remembers across sessions.
 */
export const commentSidebarOpenAtom = localStorageBackedAtom<boolean>(
  'isl.comment-sidebar-open',
  false,
);

export type GroupedComments = {
  /** PR-level comments (no filename) */
  general: DiffComment[];
  /** Comments grouped by file path */
  byFile: Map<string, DiffComment[]>;
  /** Total comment count (top-level only) */
  totalCount: number;
  /** Count of unresolved comments */
  unresolvedCount: number;
};

/**
 * Group an array of DiffComments by their filename.
 * Comments without a filename go into the 'general' bucket.
 */
export function groupCommentsByFile(comments: DiffComment[]): GroupedComments {
  const general: DiffComment[] = [];
  const byFile = new Map<string, DiffComment[]>();
  let totalCount = 0;
  let unresolvedCount = 0;

  for (const comment of comments) {
    totalCount++;
    if (comment.isResolved === false) {
      unresolvedCount++;
    }

    if (comment.filename) {
      const existing = byFile.get(comment.filename) ?? [];
      existing.push(comment);
      byFile.set(comment.filename, existing);
    } else {
      general.push(comment);
    }
  }

  return {general, byFile, totalCount, unresolvedCount};
}
