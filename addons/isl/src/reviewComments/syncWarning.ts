/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {readAtom} from '../jotaiUtils';
import {pendingCommentsAtom} from './pendingCommentsState';

/**
 * Warnings about review state that will be affected by sync.
 */
export type SyncWarnings = {
  /** Number of pending comments that may become invalid */
  pendingCommentCount: number;
  /** Number of files marked as viewed that will reset */
  viewedFileCount: number;
  /** Whether there are any warnings at all */
  hasWarnings: boolean;
};

/**
 * Check for review state that will be affected by syncing a PR.
 *
 * Pending comments may become invalid because:
 * - Rebase changes line numbers
 * - Lines may be removed or moved significantly
 *
 * Note on SYN-05 (pending comments persistence through rebase):
 * Pending comments ARE persisted in localStorage (via pendingCommentsAtom with
 * 7-day expiry). They survive the rebase operation itself. However, their line
 * numbers may no longer match the new code after rebase, making them potentially
 * invalid. The warning informs users of this risk, but comments are NOT deleted.
 *
 * Viewed files will reset because:
 * - Key format includes headHash: pr:{prNumber}:{headHash}:{filePath}
 * - New commits = new headHash = all keys invalidated
 *
 * @param prNumber - The PR number to check
 * @param headHash - Current head hash (used to find viewed file keys)
 */
export function getSyncWarnings(prNumber: string, headHash: string): SyncWarnings {
  // Check pending comments for this PR
  // pendingCommentsAtom is a function that returns an atom for the given prNumber
  const pendingComments = readAtom(pendingCommentsAtom(prNumber));
  const pendingCommentCount = pendingComments.length;

  // Check viewed files for this PR
  // Keys are stored as: isl.reviewed-files:pr:{prNumber}:{headHash}:{filePath}
  // We need to count how many files are marked as viewed
  const viewedFileCount = countViewedFilesForPR(prNumber, headHash);

  return {
    pendingCommentCount,
    viewedFileCount,
    hasWarnings: pendingCommentCount > 0 || viewedFileCount > 0,
  };
}

/**
 * Count viewed files for a PR by checking localStorage keys.
 * Keys follow format: isl.reviewed-files:pr:{prNumber}:{headHash}:{filePath}
 *
 * Note: Uses 'isl.reviewed-files:' prefix (with 's') to match reviewedFilesAtom
 * definition in ComparisonView/atoms.ts
 */
function countViewedFilesForPR(prNumber: string, headHash: string): number {
  const prefix = `isl.reviewed-files:pr:${prNumber}:${headHash}:`;
  let count = 0;

  // Iterate localStorage to find matching keys
  for (let i = 0; i < localStorage.length; i++) {
    const key = localStorage.key(i);
    if (key?.startsWith(prefix)) {
      // Check if the value is 'true' (actually viewed)
      const value = localStorage.getItem(key);
      if (value === 'true') {
        count++;
      }
    }
  }

  return count;
}

/**
 * Format warning messages for display in UI.
 */
export function formatSyncWarningMessage(warnings: SyncWarnings): string {
  const parts: string[] = [];

  if (warnings.pendingCommentCount > 0) {
    parts.push(
      `${warnings.pendingCommentCount} pending comment${warnings.pendingCommentCount === 1 ? '' : 's'} may become invalid`,
    );
  }

  if (warnings.viewedFileCount > 0) {
    parts.push(
      `${warnings.viewedFileCount} viewed file${warnings.viewedFileCount === 1 ? '' : 's'} will be unmarked`,
    );
  }

  return parts.join(' and ');
}
