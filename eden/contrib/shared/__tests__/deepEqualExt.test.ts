/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {reuseEqualObjects} from '../deepEqualExt';

describe('reuseEqualObjects', () => {
  it('makes === test work like deep equal', () => {
    const oldArray = [
      {id: 'a', value: 1},
      {id: 'b', value: 2},
      {id: 'c', value: 3},
      {id: 'z', value: 5},
    ];
    const newArray = [
      {id: 'b', value: 2},
      {id: 'a', value: 1},
      {id: 'c', value: 4},
      {id: 'x', value: 3},
    ];
    const reusedArray = reuseEqualObjects(oldArray, newArray, v => v.id);

    expect(oldArray[0]).toBe(reusedArray[1]); // 'a' - reused
    expect(oldArray[1]).toBe(reusedArray[0]); // 'b' - reused

    const objToId = new Map<object, number>();
    const toId = (obj: object): number => {
      const id = objToId.get(obj);
      if (id === undefined) {
        const newId = objToId.size;
        objToId.set(obj, newId);
        return newId;
      }
      return id;
    };

    const oldIds = oldArray.map(toId);
    const newIds = newArray.map(toId);
    const reusedIds = reusedArray.map(toId);

    expect(oldIds).toEqual([0, 1, 2, 3]);
    expect(newIds).toEqual([4, 5, 6, 7]);
    expect(reusedIds).toEqual([1, 0, 6, 7]); // 'a', 'b' are reused from oldArray; 'c', 'x' are from newArray.
  });
});
