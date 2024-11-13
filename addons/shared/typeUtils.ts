/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * Remove particular keys from an object type:
 * ```
 * Without<{foo: string, bar: string, baz: number}, 'bar' | 'baz'> => {foo: string}
 * ```
 */
export type Without<T, U> = {[P in Exclude<keyof T, keyof U>]?: never};

/**
 * Given two object types, return a type allowing keys from either one but not both
 * ```
 * ExclusiveOr({foo: string}, {bar: number}) -> allows {foo: 'a'}, {bar: 1}, but not {foo: 'a', bar: 1} or {}
 * ```
 */
export type ExclusiveOr<T, U> = T | U extends object
  ? (Without<T, U> & U) | (Without<U, T> & T)
  : T | U;

/**
 * Make every key of a type optional, and make its type undefined
 * ```
 * AllUndefined<{foo: string}> => {foo?: undefined}
 * ```
 */
export type AllUndefined<T> = {[P in keyof T]?: undefined};

/**
 * Make every key of the object NOT readonly. The opposite of Readonly<T>.
 * ```
 * {readonly foo: string} -> {foo: string}
 * ```
 */
export type Writable<T> = {-readonly [P in keyof T]: T[P]};

export type Json = string | number | boolean | null | Json[] | {[key: string]: Json};

type UnionKeys<T> = T extends unknown ? keyof T : never;
type StrictUnionHelper<T, TAll> = T extends unknown
  ? T & Partial<Record<Exclude<UnionKeys<TAll>, keyof T>, undefined>>
  : never;
/**
 * Make a union type T be a strict union by making all keys required.
 * This allows a discriminated union to have fields accessed without a cast.
 * For example,
 * ```
 * StrictUnion<{type: 'foo', foo: string} | {type: 'bar', bar: number}> => {type: 'foo' | 'bar', foo?: string, bar?: number}
 * ```
 */
export type StrictUnion<T> = StrictUnionHelper<T, T>;

/**
 * Construct a type with the properties of T except for those in type K,
 * applicable to T being a union type.
 * ```
 * UnionOmit<foo | bar, 'baz'> => Omit<foo, 'baz'> | Omit<bar, 'baz'>
 * ```
 */
export type UnionOmit<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;
