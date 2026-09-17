/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import isFreshStackPullRequestCacheEntry, {
  STACK_PULL_REQUEST_CACHE_TTL_MS,
} from './stackPullRequestCache';

describe('isFreshStackPullRequestCacheEntry', () => {
  const now = 1_000_000;

  it('accepts a recently fetched entry', () => {
    expect(isFreshStackPullRequestCacheEntry(now - 1, now)).toBe(true);
  });

  it('rejects expired and legacy entries', () => {
    expect(isFreshStackPullRequestCacheEntry(now - STACK_PULL_REQUEST_CACHE_TTL_MS, now)).toBe(
      false,
    );
    expect(isFreshStackPullRequestCacheEntry(undefined, now)).toBe(false);
  });
});
