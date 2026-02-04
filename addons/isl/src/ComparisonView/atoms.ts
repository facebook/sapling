/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Comparison} from 'shared/Comparison';

import {atom} from 'jotai';
import {ComparisonType, comparisonStringKey} from 'shared/Comparison';
import {localStorageBackedAtomFamily, writeAtom} from '../jotaiUtils';
import platform from '../platform';

export type ComparisonMode = {
  comparison: Comparison;
  visible: boolean;
  /** Optional file path to scroll to when the comparison view opens */
  scrollToFile?: string;
};
export const currentComparisonMode = atom<ComparisonMode>(
  window.islAppMode?.mode === 'comparison'
    ? {
        comparison: window.islAppMode.comparison,
        visible: true,
      }
    : {
        comparison: {type: ComparisonType.UncommittedChanges},
        visible: false,
      },
);

/** Open Comparison View for a given comparison type, optionally scrolling to a specific file */
export async function showComparison(comparison: Comparison, scrollToFile?: string) {
  if (await platform.openDedicatedComparison?.(comparison)) {
    return;
  }
  writeAtom(currentComparisonMode, {comparison, visible: true, scrollToFile});
}

export function dismissComparison() {
  writeAtom(currentComparisonMode, last => ({...last, visible: false}));
}

/**
 * Generate a stable key for a file in a comparison.
 * Key format: `{comparisonType}:{hash?}:{filePath}`
 * This ensures:
 * - Same comparison type retains reviewed state
 * - Different commits have separate reviewed states
 * - Uncommitted changes reviewed state persists until you commit
 */
export function reviewedFileKey(comparison: Comparison, filePath: string): string {
  return `${comparisonStringKey(comparison)}:${filePath}`;
}

/**
 * Generate a stable key for a file being reviewed in a PR context.
 * Key format: `pr:{prNumber}:{headHash}:{filePath}`
 *
 * This ensures:
 * - Same PR version retains reviewed state
 * - New commits (different headHash) reset reviewed state
 * - Different PRs have separate reviewed states
 */
export function reviewedFileKeyForPR(prNumber: number, headHash: string, filePath: string): string {
  return `pr:${prNumber}:${headHash}:${filePath}`;
}

/**
 * Atom family for tracking which files have been reviewed in a comparison.
 * Each file's reviewed state is stored separately in localStorage,
 * keyed by comparison + file path.
 */
export const reviewedFilesAtom = localStorageBackedAtomFamily<string, boolean>(
  'isl.reviewed-files:',
  () => false,
  14, // Expire after 14 days
);
