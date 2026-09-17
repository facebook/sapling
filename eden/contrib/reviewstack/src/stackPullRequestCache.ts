/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export const STACK_PULL_REQUEST_CACHE_TTL_MS = 60 * 1000;

export default function isFreshStackPullRequestCacheEntry(
  cachedAt: unknown,
  now = Date.now(),
): cachedAt is number {
  return (
    typeof cachedAt === 'number' &&
    cachedAt <= now &&
    now - cachedAt < STACK_PULL_REQUEST_CACHE_TTL_MS
  );
}
