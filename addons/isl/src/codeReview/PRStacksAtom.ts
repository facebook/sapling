/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffSummary} from '../types';

import {atom} from 'jotai';
import {atomWithStorage} from 'jotai/utils';
import {reviewModeAtom} from '../reviewMode';
import {allDiffSummaries} from './CodeReviewInfo';

/**
 * Stack labels stored in localStorage.
 * Maps stack ID to custom label.
 */
export const stackLabelsAtom = atomWithStorage<Record<string, string>>('isl.prStackLabels', {});

/**
 * Hidden stacks stored in localStorage.
 * Set of stack IDs that are hidden.
 */
export const hiddenStacksAtom = atomWithStorage<string[]>('isl.hiddenPrStacks', []);

/**
 * Whether to auto-hide merged stacks.
 */
export const hideMergedStacksAtom = atomWithStorage<boolean>('isl.hideMergedStacks', true);

/**
 * Whether to show only stacks authored by the current user.
 * When true, hides stacks from other authors.
 */
export const showOnlyMyStacksAtom = atomWithStorage<boolean>('isl.showOnlyMyStacks', false);

/**
 * Whether to hide stacks from bot authors (renovate, dependabot, etc).
 * Default true - bots are hidden by default.
 */
export const hideBotStacksAtom = atomWithStorage<boolean>('isl.hideBotStacks', true);

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
  /** All unique authors in this stack (for multi-author display) */
  authors: Array<{login: string; avatarUrl?: string}>;
  /** Whether all PRs in this stack are merged */
  isMerged: boolean;
  /** Whether all PRs in this stack are closed (abandoned, not merged) */
  isClosed: boolean;
  /** Count of merged PRs in the stack */
  mergedCount: number;
  /** Count of closed (abandoned) PRs in the stack */
  closedCount: number;
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

  // Process non-closed PRs first so they claim stack slots before closed PRs.
  // This prevents a closed (replaced) PR from stealing stack members that belong
  // to a newer replacement stack with different stackInfo.
  const sortedEntries = [...diffsMap.entries()].sort(([, a], [, b]) => {
    if (a.state === 'CLOSED' && b.state !== 'CLOSED') return 1;
    if (a.state !== 'CLOSED' && b.state === 'CLOSED') return -1;
    return 0;
  });

  // Process each PR and group by stack
  for (const [diffId, summary] of sortedEntries) {
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
          // Skip PRs already claimed by another stack to prevent duplicates.
          // Since we process non-closed PRs first (see sort above), open/merged
          // PRs always claim slots before closed PRs. This prevents a closed
          // (replaced) PR from stealing members of a newer replacement stack.
          if (processedPrNumbers.has(prDiffId)) {
            continue;
          }

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
        const mainAuthor = firstPr.type === 'github' ? firstPr.author : undefined;
        const mainAuthorAvatarUrl = firstPr.type === 'github' ? firstPr.authorAvatarUrl : undefined;

        // Check merge/close status
        const mergedCount = stackPrs.filter(pr => pr.state === 'MERGED').length;
        const closedCount = stackPrs.filter(pr => pr.state === 'CLOSED').length;
        const doneCount = mergedCount + closedCount;
        const isMerged = mergedCount > 0 && doneCount === stackPrs.length;
        const isClosed = closedCount > 0 && closedCount === stackPrs.length;

        stacks.push({
          id: `stack-${topPrNumber}`,
          topPrNumber,
          prs: stackPrs,
          isStack: stackPrs.length > 1,
          mainAuthor,
          mainAuthorAvatarUrl,
          authors: [],
          isMerged,
          isClosed,
          mergedCount,
          closedCount,
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
      const mainAuthorAvatarUrl = summary.type === 'github' ? summary.authorAvatarUrl : undefined;

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
        authors: [],
        isMerged,
        isClosed,
        mergedCount: isMerged ? 1 : 0,
        closedCount: isClosed ? 1 : 0,
        hasStaleAbove: false,
      });
    }
  }

  // === Branch-chain detection ===
  // Detect PRs connected by branch targeting (baseRefName matches another PR's
  // branchName) and merge them into existing stacks or form new chain-based stacks.
  // This handles PRs stacked on top of a Sapling-managed stack without footers.

  // Build branchName → diffId lookup for all PRs
  const branchToDiffId = new Map<string, string>();
  for (const [diffId, summary] of diffsMap) {
    if (summary.type === 'github' && summary.branchName) {
      branchToDiffId.set(summary.branchName, diffId);
    }
  }

  // Build child → parent and parent → children maps via baseRefName matching.
  const childToParent = new Map<string, string>();
  const parentToChildren = new Map<string, string[]>();
  for (const [diffId, summary] of diffsMap) {
    if (summary.type !== 'github' || !summary.baseRefName) continue;
    const parentId = branchToDiffId.get(summary.baseRefName);
    if (parentId != null && parentId !== diffId) {
      childToParent.set(diffId, parentId);
      const children = parentToChildren.get(parentId) ?? [];
      children.push(diffId);
      parentToChildren.set(parentId, children);
    }
  }

  // Walk descendants from a starting point in chain order.
  // Returns [immediate child, grandchild, ...].
  function collectDescendants(startId: string): string[] {
    const result: string[] = [];
    let currentId = startId;
    const visited = new Set<string>([startId]);
    while (true) {
      const children = (parentToChildren.get(currentId) ?? []).filter(
        id => !visited.has(id),
      );
      if (children.length === 0) break;
      const nextId = children[0];
      visited.add(nextId);
      result.push(nextId);
      currentId = nextId;
    }
    return result;
  }

  // Build set of diffIds already in footer-based (multi-PR) stacks
  const inFooterStack = new Set<string>();
  for (const stack of stacks) {
    if (stack.isStack) {
      for (const pr of stack.prs) {
        if (pr.type === 'github') {
          inFooterStack.add(String(pr.number));
        }
      }
    }
  }

  // Phase 1: Extend footer-based multi-PR stacks with branch-chained descendants.
  // Only process existing multi-PR stacks (not singles) so they claim descendants first.
  const alreadyMerged = new Set<string>();
  for (const stack of stacks) {
    if (!stack.isStack) continue;
    const topPr = stack.prs[0];
    if (topPr.type !== 'github') continue;
    const topDiffId = String(topPr.number);

    const descendantIds = collectDescendants(topDiffId).filter(
      id => !inFooterStack.has(id),
    );
    if (descendantIds.length === 0) continue;

    const descendants = descendantIds
      .map(id => diffsMap.get(id))
      .filter((s): s is DiffSummary => s != null);

    if (descendants.length > 0) {
      // Prepend in reverse: newest/outermost descendant at top of stack
      stack.prs.unshift(...descendants.reverse());
      for (const id of descendantIds) {
        alreadyMerged.add(id);
      }
    }
  }

  // Phase 2: Form new stacks from orphan chains (singles chaining together
  // that don't connect to any footer-based stack).
  // Walk UP from each unclaimed single to find its chain root, then build
  // the full chain from that root. This avoids duplicates because each PR
  // has exactly one parent, so each root is processed exactly once.
  const processedRoots = new Set<string>();
  for (const stack of stacks) {
    if (stack.prs.length !== 1) continue;
    const pr = stack.prs[0];
    if (pr.type !== 'github') continue;
    const diffId = String(pr.number);
    if (alreadyMerged.has(diffId)) continue;

    // Walk up to find the chain root (the PR with no parent in the system)
    let root = diffId;
    const visited = new Set<string>();
    while (childToParent.has(root) && !visited.has(root)) {
      visited.add(root);
      const parent = childToParent.get(root)!;
      if (inFooterStack.has(parent) || alreadyMerged.has(parent)) break;
      root = parent;
    }
    if (processedRoots.has(root)) continue;
    processedRoots.add(root);

    // Build full chain from root downward
    const fullChain = [root, ...collectDescendants(root)];
    // Filter out any already-merged PRs
    const chain = fullChain.filter(id => !alreadyMerged.has(id) && !inFooterStack.has(id));
    if (chain.length <= 1) continue;

    // Find the stack entry for the root single and extend it
    const rootStackIdx = stacks.findIndex(s => {
      const p = s.prs[0];
      return s.prs.length === 1 && p.type === 'github' && String(p.number) === root;
    });
    if (rootStackIdx === -1) continue;

    const rootStack = stacks[rootStackIdx];
    const chainAboveRoot = chain.filter(id => id !== root);
    const chainSummaries = chainAboveRoot
      .map(id => diffsMap.get(id))
      .filter((s): s is DiffSummary => s != null);

    if (chainSummaries.length > 0) {
      rootStack.prs.unshift(...chainSummaries.reverse());
      for (const id of chainAboveRoot) {
        alreadyMerged.add(id);
      }
    }
  }

  // Remove single-PR stacks that were merged into other stacks
  const finalStacks = stacks.filter(stack => {
    if (stack.prs.length > 1) return true;
    const pr = stack.prs[0];
    const diffId = pr.type === 'github' ? String(pr.number) : '';
    return !alreadyMerged.has(diffId);
  });

  // Recalculate derived fields for all stacks (some were modified by branch-chain merging)
  for (const stack of finalStacks) {
    recomputeStackMetadata(stack);
  }

  // Sort stacks by top PR number (descending - newest first)
  finalStacks.sort((a, b) => b.topPrNumber - a.topPrNumber);

  return finalStacks;
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
 * Recompute derived metadata fields on a PRStack from its current prs list.
 */
function recomputeStackMetadata(stack: PRStack): void {
  const firstPr = stack.prs[0];
  stack.topPrNumber = parseInt(
    firstPr.type === 'github' ? String(firstPr.number) : '0',
    10,
  );
  stack.id = stack.prs.length > 1 ? `stack-${stack.topPrNumber}` : `single-${stack.topPrNumber}`;
  stack.isStack = stack.prs.length > 1;
  stack.mainAuthor = firstPr.type === 'github' ? firstPr.author : undefined;
  stack.mainAuthorAvatarUrl =
    firstPr.type === 'github' ? firstPr.authorAvatarUrl : undefined;
  // Collect unique authors across all PRs in the stack
  const seenAuthors = new Set<string>();
  stack.authors = [];
  for (const pr of stack.prs) {
    if (pr.type === 'github' && pr.author && !seenAuthors.has(pr.author)) {
      seenAuthors.add(pr.author);
      stack.authors.push({login: pr.author, avatarUrl: pr.authorAvatarUrl});
    }
  }
  const mergedCount = stack.prs.filter(pr => pr.state === 'MERGED').length;
  const closedCount = stack.prs.filter(pr => pr.state === 'CLOSED').length;
  stack.mergedCount = mergedCount;
  stack.closedCount = closedCount;
  const doneCount = mergedCount + closedCount;
  stack.isMerged = mergedCount > 0 && doneCount === stack.prs.length;
  stack.isClosed = closedCount > 0 && closedCount === stack.prs.length;
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
    }),
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
      entries: [
        {
          prNumber: Number(currentPrNumberStr),
          headHash: currentPR.head,
          title: currentPR.title,
          isCurrent: true,
          state: currentPR.state,
        },
      ],
    };
  }

  // Build entries from the stack PRs (ordered top-to-bottom, same as left column)
  const entries = containingStack.prs
    .map(pr => {
      if (pr.type !== 'github') {
        return null;
      }
      // Use string comparison for isCurrent to avoid type mismatches
      const prNumStr = String(pr.number);
      // reviewDecision is null without branch protection; fall back to latestReviews
      let effectiveDecision: string | undefined = pr.reviewDecision;
      if (effectiveDecision == null && pr.type === 'github' && pr.latestReviews) {
        const hasChanges = pr.latestReviews.some(r => r.state === 'CHANGES_REQUESTED');
        const hasApproval = pr.latestReviews.some(r => r.state === 'APPROVED');
        if (hasChanges) {
          effectiveDecision = 'CHANGES_REQUESTED';
        } else if (hasApproval) {
          effectiveDecision = 'APPROVED';
        }
      }
      return {
        prNumber: Number(pr.number),
        headHash: pr.head,
        title: pr.title,
        isCurrent: prNumStr === currentPrNumberStr,
        state: pr.state,
        reviewDecision: effectiveDecision ?? undefined,
      };
    })
    .filter((e): e is NonNullable<typeof e> => e !== null);

  const currentIndex = entries.findIndex(e => e.isCurrent);

  return {
    currentIndex: currentIndex >= 0 ? currentIndex : 0,
    stackSize: entries.length,
    entries,
    isSinglePr: entries.length <= 1,
  };
});
