/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../types';
import type {List, Seq} from 'immutable';

import {OrderedSet as ImSet} from 'immutable';
import {SelfUpdate} from 'shared/immutableExt';

/**
 * Set of commit hashes, with extra methods.
 * Internally maintains the order of the hashes information.
 */
export class HashSet extends SelfUpdate<ImSet<Hash>> {
  constructor(set?: ImSet<Hash>) {
    super(set ?? ImSet());
  }

  static fromHashes(hashes: SetLike): HashSet {
    if (hashes == null) {
      return new HashSet(ImSet());
    } else if (hashes instanceof HashSet) {
      return hashes;
    } else if (typeof hashes === 'string') {
      return new HashSet(ImSet([hashes]));
    } else if (ImSet.isOrderedSet(hashes)) {
      return new HashSet(hashes as ImSet<Hash>);
    } else {
      return new HashSet(ImSet(hashes));
    }
  }

  toHashes(): ImSet<Hash> {
    return this.set;
  }

  toSeq(): Seq.Set<Hash> {
    return this.set.toSeq();
  }

  toList(): List<Hash> {
    return this.set.toList();
  }

  toArray(): Array<Hash> {
    return this.set.toArray();
  }

  /** Union with another set. */
  union(other: SetLike): HashSet {
    const set = this.set.union(HashSet.fromHashes(other).set);
    return new HashSet(set);
  }

  /** Interset with another set. */
  intersect(other: SetLike): HashSet {
    const set = this.set.intersect(HashSet.fromHashes(other).set);
    return new HashSet(set);
  }

  /** Remove items that exist in another set. */
  subtract(other: SetLike): HashSet {
    const set = this.set.subtract(HashSet.fromHashes(other).set);
    return new HashSet(set);
  }

  /** Test if this set contains the given hash. */
  contains(hash: Hash): boolean {
    return this.set.has(hash);
  }

  /** Reverse the order of the set. */
  reverse(): HashSet {
    return new HashSet(this.inner.reverse());
  }

  /** Convert to sorted array. Mainly for testing. */
  toSortedArray(): Array<Hash> {
    return this.set.toArray().sort();
  }

  [Symbol.iterator](): IterableIterator<Hash> {
    return this.set[Symbol.iterator]();
  }

  get size(): number {
    return this.set.size;
  }

  private get set(): ImSet<Hash> {
    return this.inner;
  }
}

/** A convenient type that converts to HashSet. `null` converts to an empty set. */
export type SetLike = HashSet | ImSet<Hash> | Iterable<Hash> | Hash | null | undefined;
