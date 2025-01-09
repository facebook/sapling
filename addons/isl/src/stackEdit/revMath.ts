/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/** Simple math utilities for branded numbers */

/** `+` but preserves the return type. */
export function next<T extends number>(rev: T, offset = 1): T {
  return (rev + offset) as T;
}

/** `-` but preserves the return type. */
export function prev<T extends number>(rev: T, offset = 1): T {
  return (rev - offset) as T;
}

/** `Math.max` but preserves the return type. */
export function max<T extends number>(...values: Array<T | number>): T {
  return Math.max(...values) as T;
}
