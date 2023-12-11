/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from './types';

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

/** Get the short 12-character hash from a full hash. */
export function short(hash: Hash): string {
  return hash.slice(0, 12);
}

export function assert(shouldBeTrue: boolean, error: string): asserts shouldBeTrue {
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

export function getWindowWidthInPixels(): number {
  if (process.env.NODE_ENV === 'test') {
    return 1000;
  }
  // Use client width and not screen width to handle embedding as an iframe.
  return document.body.clientWidth;
}

export function leftPad(val: string | number, len: number, char: string) {
  const str = val.toString();
  return `${Array(len - str.length + 1).join(char)}${str}`;
}
