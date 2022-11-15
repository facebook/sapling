/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {LRU} from '../LRU';

describe('LRU', () => {
  it('evicts oldest items after reaching the max', () => {
    const lru = new LRU(3);
    lru.set(1, 1);
    lru.set(2, 2);
    lru.set(3, 3);
    lru.set(4, 4);
    expect(lru.get(4)).toEqual(4);
    expect(lru.get(3)).toEqual(3);
    expect(lru.get(2)).toEqual(2);
    expect(lru.get(1)).toEqual(undefined);
  });

  it('refreshes items on get', () => {
    const lru = new LRU(3);
    lru.set(1, 1);
    lru.set(2, 2);
    lru.set(3, 3);
    expect(lru.get(1)).toEqual(1);
    lru.set(4, 4);
    expect(lru.get(2)).toEqual(undefined);
    expect(lru.get(3)).toEqual(3);
    expect(lru.get(4)).toEqual(4);
  });

  it('refreshes items on set', () => {
    const lru = new LRU(3);
    lru.set(1, 1);
    lru.set(2, 2);
    lru.set(3, 3);
    lru.set(4, 4);
    lru.set(1, 1.1);
    expect(lru.get(4)).toEqual(4);
    expect(lru.get(3)).toEqual(3);
    expect(lru.get(2)).toEqual(undefined);
    expect(lru.get(1)).toEqual(1.1);
  });

  it('can delete items', () => {
    const lru = new LRU(3);
    lru.set(1, 1);
    lru.set(2, 2);
    lru.set(3, 3);
    lru.delete(3);
    lru.set(4, 4);
    expect(lru.get(4)).toEqual(4);
    expect(lru.get(3)).toEqual(undefined);
    expect(lru.get(2)).toEqual(2);
    expect(lru.get(1)).toEqual(1);
  });

  it('allows storing falsey values', () => {
    const lru = new LRU(8);
    lru.set(1, null);
    lru.set(2, undefined);
    lru.set(3, '');
    lru.set(4, false);
    const emptyArray: Array<number> = [];
    lru.set(5, emptyArray);
    const emptyObject = {};
    lru.set(6, emptyObject);

    expect(lru.get(1)).toBe(null);
    expect(lru.get(2)).toBe(undefined);
    expect(lru.get(3)).toBe('');
    expect(lru.get(4)).toBe(false);
    expect(lru.get(5)).toBe(emptyArray);
    expect(lru.get(6)).toBe(emptyObject);
  });

  it('allows falsey keys', () => {
    const lru = new LRU(8);
    lru.set(null, 1);
    lru.set(undefined, 2);
    lru.set('', 3);
    lru.set(false, 4);
    const emptyArray: Array<number> = [];
    lru.set(emptyArray, 5);
    const emptyObject = {};
    lru.set(emptyObject, 6);

    expect(lru.get(null)).toBe(1);
    expect(lru.get(undefined)).toBe(2);
    expect(lru.get('')).toBe(3);
    expect(lru.get(false)).toBe(4);
    expect(lru.get(emptyArray)).toBe(5);
    expect(lru.get(emptyObject)).toBe(6);
  });

  it('undefined keys are evictable', () => {
    const lru = new LRU(2);
    lru.set(undefined, 1);
    lru.set(2, 2);
    lru.set(3, 3);

    expect(lru.get(undefined)).toBe(undefined);
  });

  it('setting an undefined value does not take space in the cache', () => {
    const lru = new LRU(2);
    lru.set(1, undefined);
    lru.set(2, null);
    lru.set(3, undefined);
    lru.set(4, null);
    lru.set(5, undefined);
    lru.set(6, undefined);

    expect(lru.get(1)).toBe(undefined);
    expect(lru.get(2)).toBe(null);
    expect(lru.get(3)).toBe(undefined);
    expect(lru.get(4)).toBe(null);
    expect(lru.get(5)).toBe(undefined);
    expect(lru.get(6)).toBe(undefined);
  });
});
