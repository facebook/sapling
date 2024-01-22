/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ValueObject} from 'immutable';

import deepEqual from 'fast-deep-equal';
import {isValueObject, is, List} from 'immutable';

type LRUKey = LRUHashKey | ValueObject;
type LRUHashKey = string | number | boolean | null | undefined | object;

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
export class LRU<K extends LRUKey, V> {
  // Implementation is based on Map having stable insertion order and O(1).
  // To support immutable objects, the cache map uses hashCode of "key" as
  // the first key, then the actual key in the nested map.
  private cache = new Map<LRUHashKey, Map<K, V>>();

  constructor(private maxItems: number, private maxHashCollision = 3) {}

  get(key: K): V | undefined {
    let result = undefined;
    const hashKey = getHashKey(key);
    const valueMap = this.cache.get(hashKey);
    if (valueMap !== undefined) {
      // Fast path: by object reference.
      const maybeValue = valueMap.get(key);
      if (maybeValue !== undefined) {
        result = maybeValue;
      } else {
        // Slower path: immutable.is
        for (const [k, v] of valueMap) {
          // The order matters. `is(key, k)` updates `key` (user-provided) to
          // the `k` (cache) reference. See `immutableExt.withSelfUpdateEquals`.
          if (is(key, k)) {
            result = v;
            break;
          }
        }
      }
      this.cache.delete(hashKey);
      this.cache.set(hashKey, valueMap);
    }
    return result;
  }

  set(key: K, value: V) {
    const hashKey = getHashKey(key);
    let valueMap = this.cache.get(hashKey);
    if (valueMap === undefined || valueMap.size >= this.maxHashCollision) {
      valueMap = new Map([[key, value]]);
    } else {
      valueMap.set(key, value);
    }
    // ensure refresh by deleting before setting
    this.cache.delete(hashKey);

    if (value !== undefined) {
      this.cache.set(hashKey, valueMap);
      if (this.cache.size > this.maxItems) {
        // evict oldest
        // iteration order guarantees oldest item is first
        const next = this.cache.keys().next();
        // undefined is a valid value, so use iterator `done` to know whether to delete or not.
        if (!next.done) {
          this.cache.delete(next.value);
        }
      }
    }
  }

  delete(key: K) {
    const hashKey = getHashKey(key);
    this.cache.delete(hashKey);
  }

  clear() {
    this.cache.clear();
  }
}

function getHashKey<K extends LRUKey>(key: K): LRUHashKey {
  // @ts-expect-error (string)?.hashCode is valid JavaScript.
  const hashCodeFunc = key?.hashCode;
  if (hashCodeFunc !== undefined) {
    return hashCodeFunc.apply(key);
  }
  return key;
}

// Neither `unknown` nor `never` works here.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AnyFunction<T> = (this: T, ...args: any[]) => any;

type CacheStats = {
  hit?: number;
  miss?: number;
  skip?: number;
};

export interface LRUWithStats extends LRU<LRUKey, unknown> {
  stats?: CacheStats;
}

export interface WithCache {
  cache: LRUWithStats;
}

/** Cache options. */
type CacheOpts<This> = {
  /**
   * If set, use the specified cache.
   *
   * Callsite can assign `cache.stats = {hit: 0, miss: 0, skip: 0}`
   * to collect statistics.
   */
  cache?: LRUWithStats;

  /**
   * If set, and cache is not set, create cache of the given size.
   * Default value: 200.
   */
  cacheSize?: number;

  /** If set, use the returned values as extra cache keys. */
  getExtraKeys?: (this: This) => unknown[];

  /**
   * Cached functions are expected to be "pure" - give same output
   * for the same inputs. If set to true, compare the cached value
   * with a fresh recalculation and throws on mismatch.
   */
  audit?: boolean;

  /**
   * Track the `cache` so it can be cleared by `clearTrackedCache`.
   * Default: true.
   */
  track?: boolean;
};

type DecoratorFunc = (target: unknown, propertyKey: string, descriptor: PropertyDescriptor) => void;

/**
 * Decorator to make a class method cached.
 *
 * This is similar to calling `cached` on the function, with
 * an auto-generated `opts.getExtraKeys` function that turns
 * `this` into extra cache keys. Immutable `this` is used
 * as the extra cache key directly. Otherwise, cachable
 * properties of `this` are used as extra cache keys.
 */
export function cached<T>(opts?: CacheOpts<T>): DecoratorFunc;

/**
 * Wraps the given function with a LRU cache.
 * Returns the wrapped function.
 *
 * If the function depends on extra inputs outside the
 * parameters, use `opts.getExtraKeys` to provide them.
 *
 * The cache can be accessed via `returnedFunction.cache`.
 *
 * Cache is used only when all parameters are cachable [1].
 * For example, if a parameter is a function or `null`,
 * then cache is only used when that parameter is `null`,
 * since functions are not cachable.
 *
 * [1]: See `isCachable` for cachable types.
 */
export function cached<T, F extends AnyFunction<T>>(func: F, opts?: CacheOpts<T>): F & WithCache;

// union of the above
export function cached<T, F extends AnyFunction<T>>(
  arg1?: F | CacheOpts<T>,
  arg2?: CacheOpts<T>,
): (F & WithCache) | DecoratorFunc {
  if (typeof arg1 === 'function') {
    // cached(func)
    return cachedFunction(arg1, arg2);
  } else {
    // @cached(opts)
    return cacheDecorator(arg1);
  }
}

const trackedCaches = new Set<WeakRef<LRUWithStats>>();

/** Clear tracked LRU caches. By default, `@cached` */
export function clearTrackedCache() {
  for (const weakRef of trackedCaches) {
    const cache = weakRef.deref();
    if (cache === undefined) {
      trackedCaches.delete(weakRef);
    } else {
      cache.clear();
    }
  }
}

function cachedFunction<T, F extends AnyFunction<T>>(func: F, opts?: CacheOpts<T>): F & WithCache {
  const cache: LRUWithStats = opts?.cache ?? new LRU(opts?.cacheSize ?? 200);
  const audit = opts?.audit ?? false;
  const getExtraKeys = opts?.getExtraKeys;
  const track = opts?.track ?? true;
  if (track) {
    trackedCaches.add(new WeakRef(cache));
  }
  const cachedFunc = function (this: T, ...args: Parameters<F>): ReturnType<F> {
    const stats = cache.stats;
    if (!args.every(isCachable)) {
      if (stats != null) {
        stats.skip = (stats.skip ?? 0) + 1;
      }
      return func.apply(this, args) as ReturnType<F>;
    }
    const cacheKey = List(getExtraKeys ? [...getExtraKeys.apply(this), ...args] : args);
    const cachedValue = cache.get(cacheKey);
    if (cachedValue !== undefined) {
      if (stats != null) {
        stats.hit = (stats.hit ?? 0) + 1;
      }
      if (audit) {
        const result = func.apply(this, args) as ReturnType<F>;
        const equal = isValueObject(result)
          ? is(result, cachedValue)
          : deepEqual(result, cachedValue);
        if (!equal) {
          const argsStr = args.map(a => a.toString()).join(', ');
          throw new Error(
            `cached value mismatch on ${func.name}(${argsStr}): cached ${cachedValue}, actual ${result}`,
          );
        }
      }
      return cachedValue as ReturnType<F>;
    }
    if (stats != null) {
      stats.miss = (stats.miss ?? 0) + 1;
    }
    const result = func.apply(this, args) as ReturnType<F>;
    cache.set(cacheKey, result);
    return result;
  };
  cachedFunc.cache = cache;
  return cachedFunc as WithCache & F;
}

// See https://www.typescriptlang.org/docs/handbook/decorators.html.
function cacheDecorator<T>(opts?: CacheOpts<T>) {
  const getExtraKeys =
    opts?.getExtraKeys ??
    function (this: T): unknown[] {
      // Use `this` as extra key if it's a value object (hash + eq).
      if (isValueObject(this)) {
        return [this];
      }
      // Scan through cachable properties.
      if (this != null && typeof this === 'object') {
        return Object.values(this).filter(isCachable);
      }
      // Give up - do not add extra cache keys.
      return [];
    };
  return function (_target: unknown, _propertyKey: string, descriptor: PropertyDescriptor) {
    const originalFunc = descriptor.value;
    descriptor.value = cachedFunction(originalFunc, {...opts, getExtraKeys});
  };
}

const cachableTypeNames = new Set([
  'number',
  'string',
  'boolean',
  'symbol',
  'bigint',
  'undefined',
  'null',
]);

/**
 * Returns true if `arg` can be used as cache keys.
 * Primitive types (string, number, boolean, null, undefined)
 * can be used as cache keys.
 * Objects can be used as cache keys if they are immutable.
 */
function isCachable(arg: unknown): boolean {
  // null is a special case, since typeof(null) returns 'object'.
  if (arg == null) {
    return true;
  }
  const typeName = typeof arg;
  if (cachableTypeNames.has(typeName)) {
    return true;
  }
  if (typeName === 'object' && isValueObject(arg)) {
    return true;
  }
  return false;
}
