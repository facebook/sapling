/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import deepEqual from 'fast-deep-equal';

/** Try to reuse objects from `oldArray` for objects with the same key and deepEqual values. */
export function reuseEqualObjects<T>(
  oldArray: Array<T>,
  newArray: Array<T>,
  keyFunc: (value: T) => string,
  equalFunc: (a: T, b: T) => boolean = deepEqual,
): Array<T> {
  const oldMap = new Map<string, T>(oldArray.map(v => [keyFunc(v), v]));
  return newArray.map(value => {
    const oldValue = oldMap.get(keyFunc(value));
    return oldValue && equalFunc(oldValue, value) ? oldValue : value;
  });
}
