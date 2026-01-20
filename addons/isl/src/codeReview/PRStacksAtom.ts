/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffSummary} from '../types';

import {atom} from 'jotai';
import {atomWithStorage} from 'jotai/utils';
import {allDiffSummaries} from './CodeReviewInfo';

/**
 * Stack labels stored in localStorage.
 * Maps stack ID to custom label.
 */
export const stackLabelsAtom = atomWithStorage<Record<string, string>>(
  'isl.prStackLabels',
  {},
);

/**
 * Hidden stacks stored in localStorage.
 * Set of stack IDs that are hidden.
 */
export const hiddenStacksAtom = atomWithStorage<string[]>(
  'isl.hiddenPrStacks',
  [],
);

/**
 * Represents a stack of PRs grouped together.
 */
export type PRStack = {
  /** Unique identifier for this stack (usually the top PR number) */
  id: string;
  /** The top PR number (first in the stack list) */
  topPrNumber: number;
  /** All PR summaries in the stack, ordered top-to-bottom (first = top of stack) */
  prs: DiffSummary[];
  /** Whether this stack has multiple PRs or is just a single PR */
  isStack: boolean;
  /** Main contributor (author) for this stack */
  mainAuthor?: string;
  /** Avatar URL of the main contributor */
  mainAuthorAvatarUrl?: string;
};

/**
 * Groups PRs into stacks based on their stackInfo.
 *
 * PRs with matching stackInfo are grouped together. PRs without stackInfo
 * are treated as single-PR stacks.
 *
 * Stack ordering:
 * - First entry in stackInfo = top of stack (newest commits)
 * - Last entry in stackInfo = closest to trunk (oldest commits)
 */
export const prStacksAtom = atom<PRStack[]>(get => {
  const allDiffs = get(allDiffSummaries);

  if (allDiffs.error || allDiffs.value == null) {
    return [];
  }

  const diffsMap = allDiffs.value;
  const stacks: PRStack[] = [];
  const processedPrNumbers = new Set<string>();

  // Process each PR and group by stack
  for (const [diffId, summary] of diffsMap.entries()) {
    if (processedPrNumbers.has(diffId)) {
      continue;
    }

    const stackInfo = getStackInfo(summary);

    if (stackInfo && stackInfo.length > 1) {
      // This PR is part of a multi-PR stack
      // Build the stack from the stackInfo
      const stackPrs: DiffSummary[] = [];
      let topPrNumber: number | null = null;

      for (const entry of stackInfo) {
        const prDiffId = String(entry.prNumber);
        const prSummary = diffsMap.get(prDiffId);

        if (prSummary) {
          stackPrs.push(prSummary);
          processedPrNumbers.add(prDiffId);

          if (topPrNumber === null) {
            topPrNumber = entry.prNumber;
          }
        }
      }

      if (stackPrs.length > 0 && topPrNumber !== null) {
        // Get main author from the first PR (top of stack)
        const firstPr = stackPrs[0];
        const mainAuthor =
          firstPr.type === 'github' ? firstPr.author : undefined;
        const mainAuthorAvatarUrl =
          firstPr.type === 'github' ? firstPr.authorAvatarUrl : undefined;

        stacks.push({
          id: `stack-${topPrNumber}`,
          topPrNumber,
          prs: stackPrs,
          isStack: stackPrs.length > 1,
          mainAuthor,
          mainAuthorAvatarUrl,
        });
      }
    } else {
      // Single PR (no stack info or single-entry stack)
      const prNumber = parseInt(diffId, 10);
      processedPrNumbers.add(diffId);

      // Get author from the PR
      const mainAuthor = summary.type === 'github' ? summary.author : undefined;
      const mainAuthorAvatarUrl =
        summary.type === 'github' ? summary.authorAvatarUrl : undefined;

      stacks.push({
        id: `single-${diffId}`,
        topPrNumber: prNumber,
        prs: [summary],
        isStack: false,
        mainAuthor,
        mainAuthorAvatarUrl,
      });
    }
  }

  // Sort stacks by top PR number (descending - newest first)
  stacks.sort((a, b) => b.topPrNumber - a.topPrNumber);

  return stacks;
});

/**
 * Extract stackInfo from a DiffSummary.
 * Only GitHub PRs have stackInfo.
 */
function getStackInfo(
  summary: DiffSummary,
): Array<{isCurrent: boolean; prNumber: number}> | undefined {
  if (summary.type === 'github' && 'stackInfo' in summary) {
    return summary.stackInfo;
  }
  return undefined;
}

/**
 * Atom to get just the count of stacks.
 */
export const prStacksCountAtom = atom<number>(get => {
  const stacks = get(prStacksAtom);
  return stacks.length;
});

/**
 * Atom to get count of multi-PR stacks (excludes single PRs).
 */
export const multiPrStacksCountAtom = atom<number>(get => {
  const stacks = get(prStacksAtom);
  return stacks.filter(s => s.isStack).length;
});
