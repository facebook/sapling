/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * Graphite (https://graphite.dev) posts a comment on each PR in a stack
 * with links to all PRs in the stack. This module provides utilities for
 * parsing those comments.
 *
 * Example Graphite comment format:
 *
 * ```
 * * **#3818** <a href="..."> 👈
 * * **#3815** <a href="...">
 * * **#3814** <a href="...">
 * * `main`
 *
 * ...
 * This stack of pull requests is managed by **Graphite**
 * ```
 */

export type GraphiteStackBody = {
  /** PR numbers in stack order (top of stack first) */
  stack: number[];
  /** Index of the current PR in the stack (marked with 👈) */
  currentStackEntry: number;
  /** Base branch name (e.g., "main") */
  baseBranch: string | null;
};

const GRAPHITE_SIGNATURE = 'This stack of pull requests is managed by';

/**
 * Check if a comment body is a Graphite stack comment.
 */
export function isGraphiteStackComment(body: string): boolean {
  return body.includes(GRAPHITE_SIGNATURE);
}

/**
 * Parse a Graphite stack comment and extract PR numbers and current position.
 * Returns null if the comment is not a valid Graphite stack comment.
 */
export function parseGraphiteStackComment(body: string): GraphiteStackBody | null {
  if (!isGraphiteStackComment(body)) {
    return null;
  }

  const stack: number[] = [];
  let currentStackEntry = -1;
  let baseBranch: string | null = null;

  // Match PR number lines: * **#123** ...
  // The 👈 emoji marks the current PR
  const prLineRegex = /^\* \*\*#(\d+)\*\*/gm;
  const baseLineRegex = /^\* `([^`]+)`/gm;

  const lines = body.split('\n');

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    // Check for PR number line
    const prMatch = /^\* \*\*#(\d+)\*\*/.exec(line);
    if (prMatch) {
      const prNumber = parseInt(prMatch[1], 10);
      stack.push(prNumber);

      // Check if this is the current PR (marked with 👈)
      if (line.includes('👈')) {
        currentStackEntry = stack.length - 1;
      }
      continue;
    }

    // Check for base branch line
    const baseMatch = /^\* `([^`]+)`/.exec(line);
    if (baseMatch) {
      baseBranch = baseMatch[1];
      continue;
    }
  }

  // Must have at least one PR and a current entry
  if (stack.length === 0) {
    return null;
  }

  // If no 👈 marker found, we can't determine current position
  if (currentStackEntry === -1) {
    return null;
  }

  return {
    stack,
    currentStackEntry,
    baseBranch,
  };
}
