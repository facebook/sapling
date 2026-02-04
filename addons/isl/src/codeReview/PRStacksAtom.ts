/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffSummary} from '../types';

import {PullRequestState} from 'isl-server/src/github/generated/graphql';
import {atom} from 'jotai';
import {atomWithStorage} from 'jotai/utils';
import {allDiffSummaries} from './CodeReviewInfo';
import {reviewModeAtom} from '../reviewMode';

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
 * Whether to auto-hide merged stacks.
 */
export const hideMergedStacksAtom = atomWithStorage<boolean>(
  'isl.hideMergedStacks',
  true,
);

/**
 * Whether to show only stacks authored by the current user.
 * When true, hides stacks from other authors.
 */
export const showOnlyMyStacksAtom = atomWithStorage<boolean>(
  'isl.showOnlyMyStacks',
  false,
);

/**
 * Whether to hide stacks from bot authors (renovate, dependabot, etc).
 * Default true - bots are hidden by default.
 */
export const hideBotStacksAtom = atomWithStorage<boolean>(
  'isl.hideBotStacks',
  true,
);

/**
 * List of known bot author patterns (case-insensitive).
 */
const BOT_AUTHOR_PATTERNS = [
  'renovate',
  'dependabot',
  'github-actions',
  'semantic-release',
  'greenkeeper',
  'snyk-bot',
  'codecov',
  'mergify',
  'netlify',
  'vercel',
  'bot',
];

/**
 * Check if an author name matches a known bot pattern.
 */
export function isBotAuthor(author: string | undefined): boolean {
  if (!author) return false;
  const lowerAuthor = author.toLowerCase();
  return BOT_AUTHOR_PATTERNS.some(
    pattern => lowerAuthor.includes(pattern) || lowerAuthor.endsWith('[bot]'),
  );
}

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
  /** Whether all PRs in this stack are merged */
  isMerged: boolean;
  /** Whether all PRs in this stack are closed (abandoned, not merged) */
  isClosed: boolean;
  /** Count of merged PRs in the stack */
  mergedCount: number;
  /**
   * True if this stack has "stale" PRs - PRs that are still open but whose
   * changes were already merged via a higher PR (merged directly on GitHub).
   * This happens when the top PR in stackInfo is missing (merged elsewhere).
   */
  hasStaleAbove: boolean;
  /** The PR number that was merged above this stack (if hasStaleAbove) */
  mergedAbovePrNumber?: number;
};

/**
 * Context for navigating between PRs in a stack during review mode.
 */
export type StackNavigationContext = {
  /** Current PR's position in stack (0 = top of stack) */
  currentIndex: number;
  /** Total PRs in stack */
  stackSize: number;
  /** Stack entries with PR details for navigation */
  entries: Array<{
    prNumber: number;
    headHash: string;
    title: string;
    isCurrent: boolean;
    state: DiffSummary['state'];
    /** Review decision: APPROVED, CHANGES_REQUESTED, REVIEW_REQUIRED, or undefined */
    reviewDecision?: string;
  }>;
  /** Whether this is a single PR (no stack navigation needed) */
  isSinglePr: boolean;
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

      // Track if the TRUE top of the stack (first in stackInfo) is missing
      // This indicates the top was merged via GitHub and lower PRs are now "stale"
      const trueTopPrNumber = stackInfo[0]?.prNumber;
      const trueTopInDiffs = trueTopPrNumber ? diffsMap.has(String(trueTopPrNumber)) : false;
      let hasStaleAbove = false;
      let mergedAbovePrNumber: number | undefined;

      for (const entry of stackInfo) {
        const prDiffId = String(entry.prNumber);
        const prSummary = diffsMap.get(prDiffId);

        if (prSummary) {
          stackPrs.push(prSummary);
          processedPrNumbers.add(prDiffId);

          if (topPrNumber === null) {
            topPrNumber = entry.prNumber;
          }
        } else if (topPrNumber === null && entry.prNumber === trueTopPrNumber) {
          // The true top is missing (likely merged) - mark stack as having stale PRs
          hasStaleAbove = true;
          mergedAbovePrNumber = trueTopPrNumber;
        }
      }

      if (stackPrs.length > 0 && topPrNumber !== null) {
        // Get main author from the first PR (top of stack)
        const firstPr = stackPrs[0];
        const mainAuthor =
          firstPr.type === 'github' ? firstPr.author : undefined;
        const mainAuthorAvatarUrl =
          firstPr.type === 'github' ? firstPr.authorAvatarUrl : undefined;

        // Check merge/close status
        const mergedCount = stackPrs.filter(pr => pr.state === 'MERGED').length;
        const closedCount = stackPrs.filter(pr => pr.state === 'CLOSED').length;
        const isMerged = mergedCount === stackPrs.length;
        const isClosed = closedCount === stackPrs.length;

        stacks.push({
          id: `stack-${topPrNumber}`,
          topPrNumber,
          prs: stackPrs,
          isStack: stackPrs.length > 1,
          mainAuthor,
          mainAuthorAvatarUrl,
          isMerged,
          isClosed,
          mergedCount,
          hasStaleAbove,
          mergedAbovePrNumber,
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

      // Check merge/close status
      const isMerged = summary.state === 'MERGED';
      const isClosed = summary.state === 'CLOSED';

      stacks.push({
        id: `single-${diffId}`,
        topPrNumber: prNumber,
        prs: [summary],
        isStack: false,
        mainAuthor,
        mainAuthorAvatarUrl,
        isMerged,
        isClosed,
        mergedCount: isMerged ? 1 : 0,
        hasStaleAbove: false,
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

/**
 * Derived atom that provides stack navigation context for the current PR in review mode.
 * Uses prStacksAtom for consistent stack data with the left column.
 * Returns null when not in review mode.
 * Returns { isSinglePr: true, ... } when PR has no stack.
 */
export const currentPRStackContextAtom = atom<StackNavigationContext | null>(get => {
  const reviewMode = get(reviewModeAtom);
  if (!reviewMode.active || !reviewMode.prNumber) {
    return null;
  }

  const diffs = get(allDiffSummaries);
  if (diffs.error || !diffs.value) {
    return null;
  }

  const currentPrNumberStr = String(reviewMode.prNumber);
  const currentPR = diffs.value.get(reviewMode.prNumber);

  // Find the stack containing this PR from prStacksAtom (same source as left column)
  const stacks = get(prStacksAtom);
  const containingStack = stacks.find(stack =>
    stack.prs.some(pr => {
      // Use string comparison to avoid type mismatches
      const prNumStr = pr.type === 'github' ? String(pr.number) : String(pr);
      return prNumStr === currentPrNumberStr;
    })
  );

  if (!containingStack || !containingStack.isStack) {
    // Single PR - no stack navigation
    if (!currentPR || currentPR.type !== 'github') {
      return null;
    }
    return {
      isSinglePr: true,
      currentIndex: 0,
      stackSize: 1,
      entries: [{
        prNumber: Number(currentPrNumberStr),
        headHash: currentPR.head,
        title: currentPR.title,
        isCurrent: true,
        state: currentPR.state,
      }],
    };
  }

  // Build entries from the stack PRs (ordered top-to-bottom, same as left column)
  const entries = containingStack.prs.map(pr => {
    if (pr.type !== 'github') {
      return null;
    }
    // Use string comparison for isCurrent to avoid type mismatches
    const prNumStr = String(pr.number);
    return {
      prNumber: Number(pr.number),
      headHash: pr.head,
      title: pr.title,
      isCurrent: prNumStr === currentPrNumberStr,
      state: pr.state,
      reviewDecision: pr.reviewDecision,
    };
  }).filter((e): e is NonNullable<typeof e> => e !== null);

  const currentIndex = entries.findIndex(e => e.isCurrent);

  return {
    currentIndex: currentIndex >= 0 ? currentIndex : 0,
    stackSize: entries.length,
    entries,
    isSinglePr: entries.length <= 1,
  };
});
