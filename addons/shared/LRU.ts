/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {OrderedMap} from 'immutable';

/**
 * Simple least-recently-used cache which holds at most `max` entries.
 *
 * The map uses `Immutable.is` [1] instead of `Object.is` to compare keys.
 * So it can support complex key types (ex. a tuple of immutable
 * collections and other types). Right now this is done by using data
 * structures from `immutable`. If the native `Map` supports customizing
 * the compare function we can replace `Object.is` with `Immutable.is`
 * and use native `Map` for performance.
 *
 * [1]: https://immutable-js.com/docs/v4.3.0/is()/.
 */
export class LRU<K, V> {
  // Implementation is based on Map having stable insertion order and O(1) insertion
  private cache = OrderedMap<K, V>();

  constructor(private maxItems: number) {}

  get(key: K): V | undefined {
    const val = this.cache.get(key);
    if (val === undefined) {
      return undefined;
    }

    this.cache = this.cache.withMutations(origCache => {
      let cache = origCache;
      // refresh by re-inserting
      cache = cache.delete(key);
      cache = cache.set(key, val);
      return cache;
    });

    return val;
  }

  set(key: K, value: V) {
    this.cache = this.cache.withMutations(origCache => {
      let cache = origCache;

      if (cache.has(key)) {
        // ensure refresh by deleting before setting
        cache = cache.delete(key);
      }

      if (value !== undefined) {
        // `set(key, undefined)` is indistinguishable from `key` not being in the cache,
        // as far as you can tell from `get(key)`.
        // Save a bit of space by not re-inserting into the cache after deleting.
        cache = cache.set(key, value);
      }

      if (cache.size > this.maxItems) {
        // evict oldest
        // iteration order guarantees oldest item is first
        const next = cache.keys().next();
        // undefined is a valid value, so use iterator `done` to know whether to delete or not.
        if (!next.done) {
          cache = cache.delete(next.value);
        }
      }

      return cache;
    });
  }

  delete(key: K) {
    this.cache = this.cache.delete(key);
  }

  clear() {
    this.cache = this.cache.clear();
  }
}
