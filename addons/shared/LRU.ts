/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * Simple least-recently-used cache which holds at most `max` entries.
 */
export class LRU<K, V> {
  // Implementation is based on Map having stable insertion order and O(1) insertion
  private cache = new Map<K, V>();

  constructor(private maxItems: number) {}

  get(key: K): V | undefined {
    const val = this.cache.get(key);
    if (val === undefined) {
      return undefined;
    }

    // refresh by re-inserting
    this.cache.delete(key);
    this.cache.set(key, val);

    return val;
  }

  set(key: K, value: V) {
    if (this.cache.has(key)) {
      // ensure refresh by deleting before setting
      this.cache.delete(key);
    }

    if (value === undefined) {
      // `set(key, undefined)` is indistinguishable from `key` not being in the cache,
      // as far as you can tell from `get(key)`.
      // Save a bit of space by not re-inserting into the cache after deleting.
      return;
    }

    this.cache.set(key, value);

    if (this.cache.size > this.maxItems) {
      // evict oldest
      this.deleteOldestKey();
    }
  }

  delete(key: K) {
    this.cache.delete(key);
  }

  clear() {
    this.cache.clear();
  }

  private deleteOldestKey() {
    // iteration order guarantees oldest item is first
    const next = this.cache.keys().next();
    // undefined is a valid value, so use iterator `done` to know whether to delete or not.
    if (!next.done) {
      this.cache.delete(next.value);
    }
  }
}
