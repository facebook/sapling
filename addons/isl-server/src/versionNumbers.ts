/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/** An array of parts of the version, like [Major, Minor, Subminor] */
export type ParsedVersion = Array<number>;

/** Comparator for parsed version. Return -1 if a < b, 1 if a > b, and 0 if a == b */
export function compareVersions(a: ParsedVersion, b: ParsedVersion): -1 | 0 | 1 {
  if (a.length === 0 && b.length === 0) {
    return 0;
  }
  if (b.length === 0) {
    return 1;
  }
  if (a.length === 0) {
    return -1;
  }

  return a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : compareVersions(a.slice(1), b.slice(1));
}

/**
 * Given a version ordinal label like V1, V0.1, V0.10 etc, extract the parts like [1], [0, 1], [0, 10] etc
 * This IGNORES any leading/trailing non-numeric parts.
 */
export function parseVersionParts(ordinal: string): ParsedVersion {
  try {
    const numbers = /^[a-zA-Z\-_]*(\d+(?:\.\d+)*)[a-zA-Z\-_]*$/.exec(ordinal);
    return numbers?.[1].split('.').map(part => parseInt(part, 10)) ?? [];
  } catch {
    return [];
  }
}
