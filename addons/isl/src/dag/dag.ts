/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../types';
import type {RecordOf} from 'immutable';

import {Map as ImMap, Record, List} from 'immutable';
import {SelfUpdate} from 'shared/immutableExt';

/**
 * Partial commit graph with query and edit operations.
 * Internally maintains a "parent -> child" mapping for efficient queries.
 */
export class Dag<C extends HashWithParents> extends SelfUpdate<DagRecord<C>> {
  constructor(record?: DagRecord<C>) {
    super(record ?? (EMPTY_DAG_RECORD as DagRecord<C>));
  }

  // Edit

  /**
   * Add commits. Parents do not have to be added first.
   * If a commit with the same hash already exists, it will be replaced.
   */
  add(commits: Iterable<C>): Dag<C> {
    const commitArray = [...commits];
    const dag = this.remove(commitArray.map(c => c.hash));
    let {childMap, infoMap} = dag;
    for (const commit of commitArray) {
      commit.parents.forEach(p => {
        const children = childMap.get(p);
        const child = commit.hash;
        const newChildren =
          children == null
            ? List([child])
            : children.contains(child)
            ? children
            : children.push(child);
        childMap = childMap.set(p, newChildren);
      });
      infoMap = infoMap.set(commit.hash, commit);
    }
    const record = dag.inner.merge({infoMap, childMap});
    return new Dag(record);
  }

  /** Remove commits by hash. Descendants are not removed automatically. */
  remove(hashes: Iterable<Hash>): Dag<C> {
    let {childMap, infoMap} = this;
    for (const hash of hashes) {
      const commit = this.get(hash);
      if (commit == undefined) {
        continue;
      }
      commit.parents.forEach(p => {
        const children = childMap.get(p);
        if (children != null) {
          const newChildren = children.filter(h => h !== hash);
          childMap = childMap.set(p, newChildren);
        }
      });
      infoMap = infoMap.remove(hash);
    }
    const record = this.inner.merge({infoMap, childMap});
    return new Dag(record);
  }

  /** A callback form of remove() and add(). */
  replaceWith(
    hashes: Iterable<Hash>,
    replaceFunc: (hash: Hash, commit: C | undefined) => C | undefined,
  ): Dag<C> {
    const hashArray = [...hashes];
    return this.remove(hashArray).add(
      hashArray.map(h => replaceFunc(h, this.get(h))).filter(c => c != undefined) as C[],
    );
  }

  // Basic query

  get(hash: Hash | undefined | null): Readonly<C> | undefined {
    return hash == null ? undefined : this.infoMap.get(hash);
  }

  has(hash: Hash | undefined | null): boolean {
    return this.get(hash) !== undefined;
  }

  [Symbol.iterator](): IterableIterator<Hash> {
    return this.infoMap.keys();
  }

  values(): Iterable<Readonly<C>> {
    return this.infoMap.values();
  }

  /** Get parent hashes. Only return hashes present in this.infoMap. */
  parentHashes(hash: Hash): Readonly<Hash[]> {
    return this.infoMap.get(hash)?.parents?.filter(p => this.infoMap.has(p)) ?? [];
  }

  /** Get child hashes. Only return hashes present in this.infoMap. */
  childHashes(hash: Hash): List<Hash> {
    if (!this.infoMap.has(hash)) {
      return EMPTY_LIST;
    }
    return this.childMap.get(hash) ?? EMPTY_LIST;
  }

  // Delegates

  get infoMap(): ImMap<Hash, Readonly<C>> {
    return this.inner.infoMap;
  }

  get childMap(): ImMap<Hash, List<Hash>> {
    return this.inner.childMap;
  }
}

/** Minimal fields needed to be used in commit graph structures. */
export interface HashWithParents {
  hash: Hash;
  parents: Hash[];
  // TODO: We might want "ancestors" to express distant parent relationships.
  // However, sl does not yet have a way to expose that information.
}

type DagProps<C extends HashWithParents> = {
  infoMap: ImMap<Hash, Readonly<C>>;
  // childMap is derived from infoMap.
  childMap: ImMap<Hash, List<Hash>>;
};

const DagRecord = Record<DagProps<HashWithParents>>({
  infoMap: ImMap(),
  childMap: ImMap(),
});
type DagRecord<C extends HashWithParents> = RecordOf<DagProps<C>>;

const EMPTY_DAG_RECORD = DagRecord();
const EMPTY_LIST = List<Hash>();
