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
