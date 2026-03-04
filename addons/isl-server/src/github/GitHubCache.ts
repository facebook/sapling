/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

type CacheEntry<T = unknown> = {
  data: T;
  timestamp: number;
  ttlMs: number;
};

/**
 * Simple in-memory TTL cache for GitHub API responses.
 * Supports stale-while-revalidate pattern via getStale().
 * Clears on server restart (no disk persistence).
 */
export class GitHubCache {
  private store = new Map<string, CacheEntry>();

  /** Get data if cache hit and not expired. Returns undefined if miss or expired. */
  get<T = unknown>(key: string): T | undefined {
    const entry = this.store.get(key);
    if (entry == null) {
      return undefined;
    }
    if (Date.now() - entry.timestamp > entry.ttlMs) {
      return undefined;
    }
    return entry.data as T;
  }

  /** Get data even if expired (for stale-while-revalidate). Returns undefined only on miss. */
  getStale<T = unknown>(key: string): T | undefined {
    const entry = this.store.get(key);
    return entry != null ? (entry.data as T) : undefined;
  }

  /** Store data with a TTL in milliseconds. */
  set<T = unknown>(key: string, data: T, ttlMs: number): void {
    this.store.set(key, {data, timestamp: Date.now(), ttlMs});
  }

  /** Check if a key is expired or missing. */
  isExpired(key: string): boolean {
    const entry = this.store.get(key);
    if (entry == null) {
      return true;
    }
    return Date.now() - entry.timestamp > entry.ttlMs;
  }

  /** Remove a specific key. */
  invalidate(key: string): void {
    this.store.delete(key);
  }

  /** Remove all entries. */
  clear(): void {
    this.store.clear();
  }
}
