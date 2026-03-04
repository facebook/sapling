/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {GitHubCache} from '../github/GitHubCache';

describe('GitHubCache', () => {
  let cache: GitHubCache;

  beforeEach(() => {
    cache = new GitHubCache();
    jest.useFakeTimers();
  });

  afterEach(() => {
    jest.useRealTimers();
  });

  it('returns undefined for cache miss', () => {
    expect(cache.get('nonexistent')).toBeUndefined();
  });

  it('stores and retrieves data', () => {
    cache.set('key', {hello: 'world'}, 5000);
    expect(cache.get('key')).toEqual({hello: 'world'});
  });

  it('returns data before TTL expires', () => {
    cache.set('key', 'value', 5000);
    jest.advanceTimersByTime(4999);
    expect(cache.get('key')).toBe('value');
  });

  it('returns undefined after TTL expires', () => {
    cache.set('key', 'value', 5000);
    jest.advanceTimersByTime(5001);
    expect(cache.get('key')).toBeUndefined();
  });

  it('getStale returns data even after TTL expires', () => {
    cache.set('key', 'value', 5000);
    jest.advanceTimersByTime(10000);
    expect(cache.getStale('key')).toBe('value');
  });

  it('isExpired returns true after TTL', () => {
    cache.set('key', 'value', 5000);
    jest.advanceTimersByTime(5001);
    expect(cache.isExpired('key')).toBe(true);
  });

  it('isExpired returns false before TTL', () => {
    cache.set('key', 'value', 5000);
    jest.advanceTimersByTime(1000);
    expect(cache.isExpired('key')).toBe(false);
  });

  it('isExpired returns true for missing keys', () => {
    expect(cache.isExpired('missing')).toBe(true);
  });

  it('invalidate removes a key', () => {
    cache.set('key', 'value', 5000);
    cache.invalidate('key');
    expect(cache.get('key')).toBeUndefined();
    expect(cache.getStale('key')).toBeUndefined();
  });

  it('clear removes all keys', () => {
    cache.set('a', 1, 5000);
    cache.set('b', 2, 5000);
    cache.clear();
    expect(cache.get('a')).toBeUndefined();
    expect(cache.get('b')).toBeUndefined();
  });
});
