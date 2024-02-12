/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/* Common types useful in source control. */

/** Hex commit hash. */
export type Hash = string;

/** Path in the repository. Uses '/' path separator on all platforms. */
export type RepoPath = string;

/**
 * Timestamp with timezone. [UTC unix timestamp in seconds, timezone offset].
 *
 * Use `sl dbsh` to check the format. For example:
 *
 * ```
 * In [1]: util.parsedate('now')
 * Out[1]: (1679592842, 25200)
 *
 * In [2]: util.parsedate('May 1 +0800')
 * Out[2]: (1682870400, -28800)
 * ```
 */
export type DateTuple = [number, number];

/** Author with email. For example, "Test <test@example.com>". */
export type Author = string;
