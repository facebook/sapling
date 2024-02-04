/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../../types';

import {HashSet} from '../set';

describe('HashSet', () => {
  const setAb = HashSet.fromHashes(['a', 'b']);
  const setBc = HashSet.fromHashes(['b', 'c']);

  it('intersect()', () => {
    const set = setAb.intersect(setBc);
    expect(set.toHashes().toArray().sort()).toEqual(['b']);
  });

  it('union()', () => {
    const set = setAb.union(setBc);
    expect(set.toHashes().toArray().sort()).toEqual(['a', 'b', 'c']);
  });

  it('substract()', () => {
    const set = setAb.subtract(setBc);
    expect(set.toHashes().toArray().sort()).toEqual(['a']);
  });

  it('implements Iterator', () => {
    const hashes: Array<Hash> = [];
    for (const hash of setAb) {
      hashes.push(hash);
    }
    hashes.sort();
    expect(hashes).toEqual(['a', 'b']);
  });
});
