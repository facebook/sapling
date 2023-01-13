/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * Given a multi-line string, return the first line excluding '\n'.
 * If no newlines in the string, return the whole string.
 */
export function firstLine(s: string): string {
  return s.split('\n', 1)[0];
}

export function firstOfIterable<T>(it: IterableIterator<T>): T | undefined {
  return it.next().value;
}

export function assert(shouldBeTrue: boolean, error: string): void {
  if (!shouldBeTrue) {
    throw new Error(error);
  }
}

export type NonNullReactElement = React.ReactElement | React.ReactFragment;

/**
 * name of the isl platform being used,
 * for example 'browser' or 'vscode'.
 * Note: This is exposed outisde of isl/platform.ts to prevent import cycles.
 */
export function islPlatformName(): string {
  return window.islPlatform?.platformName ?? 'browser';
}
