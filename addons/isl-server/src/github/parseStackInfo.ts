/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * Represents a single entry in a PR stack.
 */
export type StackEntry = {
  /** True if this is the current PR (marked with arrow in footer) */
  isCurrent: boolean;
  /** The PR number */
  prNumber: number;
};

/**
 * The Sapling footer marker that indicates the start of stack info.
 */
const SAPLING_FOOTER_MARKER = '[//]: # (BEGIN SAPLING FOOTER)';

/**
 * Legacy marker for backward compatibility.
 */
const LEGACY_STACK_MARKER = 'Stack created with [Sapling]';

/**
 * Regex to match stack entries in the PR body.
 * Matches lines like:
 *   * #123
 *   * #123 (2 commits)
 *   * __->__ #42
 *   * __->__ #42 (3 commits)
 */
const STACK_ENTRY_REGEX = /^\* (__->__ )?#(\d+).*$/;

/**
 * Parse stack info from PR body. Matches the Sapling footer format:
 *
 * Stack ordering (top-to-bottom as it appears in the PR body):
 * - First entry = top of stack (newest commits)
 * - Last entry = closest to trunk (oldest commits)
 *
 * Example footer:
 * ```
 * ---
 * [//]: # (BEGIN SAPLING FOOTER)
 * Stack created with [Sapling](https://sapling-scm.com). Best reviewed with [ReviewStack](...).
 * * #125
 * * __->__ #124  ← current PR (marked with arrow)
 * * #123
 * * #122         ← closest to trunk (bottom of stack)
 * ```
 *
 * @param body The PR body text
 * @returns Array of stack entries in same order (top-to-bottom), or null if no stack info found
 */
export function parseStackInfo(body: string): StackEntry[] | null {
  if (!body) {
    return null;
  }

  const lines = body.split(/\r?\n/);
  let inStackList = false;
  const stackEntries: StackEntry[] = [];

  for (const line of lines) {
    if (lineHasStackListMarker(line)) {
      inStackList = true;
      continue;
    }

    if (inStackList) {
      const match = STACK_ENTRY_REGEX.exec(line);
      if (match) {
        const [, arrow, number] = match;
        stackEntries.push({
          isCurrent: Boolean(arrow),
          prNumber: parseInt(number, 10),
        });
      } else if (stackEntries.length > 0) {
        // We've reached the end of the list (non-matching line after entries)
        break;
      }
    }
  }

  return stackEntries.length > 0 ? stackEntries : null;
}

/**
 * Check if a line indicates the start of the stack list.
 */
function lineHasStackListMarker(line: string): boolean {
  return line === SAPLING_FOOTER_MARKER || line.startsWith(LEGACY_STACK_MARKER);
}

/**
 * Get the index of the current PR in the stack.
 * @param stackEntries The parsed stack entries
 * @returns The index of the current PR, or -1 if not found
 */
export function getCurrentPrIndex(stackEntries: StackEntry[]): number {
  return stackEntries.findIndex(entry => entry.isCurrent);
}

/**
 * Get all PR numbers in the stack.
 * @param stackEntries The parsed stack entries
 * @returns Array of PR numbers in stack order (top-to-bottom)
 */
export function getStackPrNumbers(stackEntries: StackEntry[]): number[] {
  return stackEntries.map(entry => entry.prNumber);
}
