/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {WithPreviewType} from '../previews';
import type {CommitInfo, Hash} from '../types';
import type {SetLike} from './set';
import type {RecordOf, List} from 'immutable';

import {CommitPreview} from '../previews';
import {BaseDag, type SortProps} from './base_dag';
import {MutationDag} from './mutation_dag';
import {HashSet} from './set';
import {Record, Map as ImMap, Set as ImSet} from 'immutable';
import {SelfUpdate} from 'shared/immutableExt';
import {notEmpty, splitOnce, unwrap} from 'shared/utils';

/**
 * Main commit graph type used for preview calculation and queries.
 *
 * See `BaseDag` docstring for differences with a traditional source
 * control dag.
 *
 * A commit is associated with the `Info` type. This enables the class
 * to provide features not existed in `BaseDag`, like:
 * - Lookup by name (bookmark, '.', etc) via resolve().
 * - Phase related queries like public() and draft().
 * - Mutation related queries like obsolete().
 * - High-level operations like rebase(), cleanup().
 */
export class Dag extends SelfUpdate<CommitDagRecord> {
  constructor(record?: CommitDagRecord) {
    super(record ?? EMPTY_DAG_RECORD);
  }

  static fromDag(commitDag?: BaseDag<Info>, mutationDag?: MutationDag): Dag {
    return new Dag(CommitDagRecord({commitDag, mutationDag}));
  }

  // Delegates

  get commitDag(): BaseDag<Info> {
    return this.inner.commitDag;
  }

  get mutationDag(): MutationDag {
    return this.inner.mutationDag;
  }

  private withCommitDag(f: (dag: BaseDag<Info>) => BaseDag<Info>): Dag {
    const newCommitDag = f(this.commitDag);
    const newRecord = this.inner.set('commitDag', newCommitDag);
    return new Dag(newRecord);
  }

  // Basic edit

  add(commits: Iterable<Info>): Dag {
    // When adding commits, also update the mutationDag.
    // Assign `seqNumber` (insertion order) to help sorting commits later.
    // The seqNumber is the same for all `commits` so the order does not matter.
    const seqNumber = this.inner.nextSeqNumber;
    const commitArray = [...commits].map(c => ({...c, seqNumber: c.seqNumber ?? seqNumber}));
    const oldNewPairs = new Array<[Hash, Hash]>();
    for (const info of commitArray) {
      info.closestPredecessors?.forEach(p => oldNewPairs.push([p, info.hash]));
      if (info.successorInfo != null) {
        oldNewPairs.push([info.hash, info.successorInfo.hash]);
      }
    }

    // Update nameMap.
    const toDelete = commitArray.map(c => this.get(c.hash)).filter(notEmpty);
    const nameMap = calculateNewNameMap(this.inner.nameMap, toDelete, commitArray);

    // Update other fields.
    const commitDag = this.commitDag.add(commitArray);
    const mutationDag = this.mutationDag.addMutations(oldNewPairs);
    const nextSeqNumber = seqNumber + 1;
    const record = this.inner.merge({
      commitDag,
      mutationDag,
      nameMap,
      nextSeqNumber,
    });
    return new Dag(record);
  }

  /** See MutationDag.addMutations. */
  addMutations(oldNewPairs: Iterable<[Hash, Hash]>): Dag {
    const newMutationDag = this.mutationDag.addMutations(oldNewPairs);
    const newRecord = this.inner.set('mutationDag', newMutationDag);
    return new Dag(newRecord);
  }

  remove(set: SetLike): Dag {
    // When removing commits, don't remove them from the mutationDag intentionally.
    const hashSet = HashSet.fromHashes(set);
    const toDelete = this.getBatch(hashSet.toArray());
    const nameMap = calculateNewNameMap(this.inner.nameMap, toDelete, []);
    const commitDag = this.commitDag.remove(hashSet);
    const record = this.inner.merge({
      commitDag,
      nameMap,
    });
    return new Dag(record);
  }

  /** A callback form of remove() and add(). */
  replaceWith(set: SetLike, replaceFunc: (h: Hash, c?: Info) => Info | undefined): Dag {
    const hashSet = HashSet.fromHashes(set);
    const hashes = hashSet.toHashes();
    return this.remove(this.present(set)).add(
      hashes.map(h => replaceFunc(h, this.get(h))).filter(c => c != undefined) as Iterable<Info>,
    );
  }

  // Basic query

  get(hash: Hash | undefined | null): Info | undefined {
    return this.commitDag.get(hash);
  }

  getBatch(hashes: Array<Hash>): Array<Info> {
    return hashes.map(h => this.get(h)).filter(notEmpty);
  }

  has(hash: Hash | undefined | null): boolean {
    return this.commitDag.has(hash);
  }

  [Symbol.iterator](): IterableIterator<Hash> {
    return this.commitDag[Symbol.iterator]();
  }

  values(): Iterable<Readonly<Info>> {
    return this.commitDag.values();
  }

  parentHashes(hash: Hash): Readonly<Hash[]> {
    return this.commitDag.parentHashes(hash);
  }

  childHashes(hash: Hash): List<Hash> {
    return this.commitDag.childHashes(hash);
  }

  // High-level query

  /** Return hashes present in this dag. */
  present(set: SetLike): HashSet {
    return this.commitDag.present(set);
  }

  parents(set: SetLike): HashSet {
    return this.commitDag.parents(set);
  }

  children(set: SetLike): HashSet {
    return this.commitDag.children(set);
  }

  ancestors(set: SetLike, props?: {within?: SetLike}): HashSet {
    return this.commitDag.ancestors(set, props);
  }

  descendants(set: SetLike, props?: {within?: SetLike}): HashSet {
    return this.commitDag.descendants(set, props);
  }

  range(roots: SetLike, heads: SetLike): HashSet {
    return this.commitDag.range(roots, heads);
  }

  roots(set: SetLike): HashSet {
    return this.commitDag.roots(set);
  }

  heads(set: SetLike): HashSet {
    return this.commitDag.heads(set);
  }

  gca(set1: SetLike, set2: SetLike): HashSet {
    return this.commitDag.gca(set1, set2);
  }

  isAncestor(ancestor: Hash, descendant: Hash): boolean {
    return this.commitDag.isAncestor(ancestor, descendant);
  }

  filter(predicate: (commit: Readonly<Info>) => boolean, set?: SetLike): HashSet {
    return this.commitDag.filter(predicate, set);
  }

  // Sort

  sortAsc(set: SetLike, props?: SortProps<Info>): Array<Hash> {
    return this.commitDag.sortAsc(set, {compare: sortAscCompare, ...props});
  }

  sortDesc(set: SetLike, props?: SortProps<Info>): Array<Hash> {
    return this.commitDag.sortDesc(set, props);
  }

  // Filters

  obsolete(set?: SetLike): HashSet {
    return this.filter(c => c.successorInfo != null, set);
  }

  public_(set?: SetLike): HashSet {
    return this.filter(c => c.phase === 'public', set);
  }

  draft(set?: SetLike): HashSet {
    return this.filter(c => (c.phase ?? 'draft') === 'draft', set);
  }

  merge(set?: SetLike): HashSet {
    return this.commitDag.merge(set);
  }

  // Edit APIs that are less generic, require `C` to be `CommitInfo`.

  /** Bump the timestamp of descendants(set) to "now". */
  touch(set: SetLike, includeDescendants = true): Dag {
    const affected = includeDescendants ? this.descendants(set) : set;
    return this.replaceWith(affected, (_h, c) => {
      return c && {...c, date: new Date()};
    });
  }

  /**
   * Remove obsoleted commits that no longer have non-obsoleted descendants.
   * If `startHeads` is not set, scan all obsoleted draft heads. Otherwise,
   * limit the scan to the given heads.
   */
  cleanup(startHeads?: SetLike): Dag {
    // ancestors(".") are not obsoleted.
    const obsolete = this.obsolete().subtract(this.ancestors(this.resolve('.')?.hash));
    // Don't trust `startHeads` as obsoleted draft heads, so we calcualte it anyway.
    let heads = this.heads(this.draft()).intersect(obsolete);
    if (startHeads !== undefined) {
      heads = heads.intersect(HashSet.fromHashes(startHeads));
    }
    const toRemove = this.ancestors(heads, {within: obsolete});
    return this.remove(toRemove);
  }

  /**
   * Attempt to rebase `srcSet` to `dest` for preview use-case.
   * Handles case that produces "orphaned" or "obsoleted" commits.
   * Does not handle:
   * - copy 'x amended to y' relation when x and y are both being rebased.
   * - skip rebasing 'x' if 'x amended to y' and 'y in ancestors(dest)'.
   */
  rebase(srcSet: SetLike, dest: Hash | undefined): Dag {
    let src = HashSet.fromHashes(srcSet);
    // x is already rebased, if x's parent is dest or 'already rebased'.
    // dest--a--b--c--d--e: when rebasing a+b+d+e to dest, only a+b are already rebased.
    const alreadyRebased = this.descendants(dest, {within: src});
    // Skip already rebased, and skip non-draft commits.
    src = this.draft(src.subtract(alreadyRebased));
    // Nothing to rebase?
    if (dest == null || src.size === 0) {
      return this;
    }
    // Rebase is not simply moving `roots(src)` to `dest`. Consider graph 'a--b--c--d',
    // 'rebase -r a+b+d -d dest' produces 'dest--a--b--d' and 'a(obsoleted)--b(obsoleted)--c':
    // - The new parent of 'd' is 'b', not 'dest'.
    // - 'a' and 'b' got duplicated.
    const srcRoots = this.roots(src); // a, d
    const orphaned = this.range(src, this.draft()).subtract(src); // c
    const duplicated = this.ancestors(orphaned).intersect(src); // a, b
    const maybeSuccHash = (h: Hash) => (duplicated.contains(h) ? `${REBASE_SUCC_PREFIX}${h}` : h);
    const date = new Date();
    const newParents = (h: Hash): Hash[] => {
      const directParents = this.parents(h);
      let parents = directParents.intersect(src);
      if (parents.size === 0) {
        parents = this.heads(this.ancestors(directParents).intersect(src));
      }
      return parents.size === 0 ? [dest] : parents.toHashes().map(maybeSuccHash).toArray();
    };
    const toCleanup = this.parents(srcRoots);
    return this.replaceWith(src.union(duplicated.toHashes().map(maybeSuccHash)), (h, c) => {
      const isSucc = h.startsWith(REBASE_SUCC_PREFIX);
      const pureHash = isSucc ? h.substring(REBASE_SUCC_PREFIX.length) : h;
      const isPred = !isSucc && duplicated.contains(h);
      const isRoot = srcRoots.contains(pureHash);
      const info = unwrap(isSucc ? this.get(pureHash) : c);
      // Reset the seqNumber so the rebase preview tends to show as right-most branches.
      const newInfo: Partial<Info> = {seqNumber: undefined};
      if (isPred) {
        // For "predecessors" (ex. a(obsoleted)), keep hash unchanged
        // so orphaned commits (c) don't move. Update successorInfo.
        const succHash = maybeSuccHash(pureHash);
        newInfo.successorInfo = {hash: succHash, type: 'rebase'};
      } else {
        // Set date, parents, previewType.
        newInfo.date = date;
        newInfo.parents = newParents(pureHash);
        newInfo.previewType = isRoot
          ? CommitPreview.REBASE_OPTIMISTIC_ROOT
          : CommitPreview.REBASE_OPTIMISTIC_DESCENDANT;
        // Set predecessor info for successors.
        if (isSucc) {
          newInfo.closestPredecessors = [pureHash];
          newInfo.hash = h;
        }
      }
      return {...info, ...newInfo};
    }).cleanup(toCleanup);
  }

  /**
   * Force the disconnected public commits to be connected to each other
   * in chronological order.
   *
   * This is "incorrect" but we don't get the info from `sl log` yet.
   *
   * Useful to reason about ancestory relations. For example, to filter
   * out rebase destinations (ex. remote/stable) that are backwards,
   * we want ancestors(rebase_src) to include public commits like
   * remote/stable.
   */
  forceConnectPublic(): Dag {
    // Not all public commits need this "fix". Only consider the "roots".
    const toFix = this.roots(this.public_());
    const sorted = toFix
      .toList()
      .sortBy(h => this.get(h)?.date.valueOf() ?? 0)
      .toArray();
    const parentPairs: Array<[Hash, Hash]> = sorted.flatMap((h, i) =>
      i === 0 ? [] : [[h, sorted[i - 1]]],
    );
    const parentMap = new Map<Hash, Hash>(parentPairs);
    return this.replaceWith(toFix, (h, c) => {
      const newParent = parentMap.get(h);
      if (c == null || newParent == null) {
        return c;
      }
      return {...c, parents: [...c.parents, newParent], ancestors: [newParent]};
    });
  }

  // Query APIs that are less generic, require `C` to be `CommitInfo`.

  /** All visible successors recursively, including `set`. */
  successors(set: SetLike): HashSet {
    return this.mutationDag.range(set, this);
  }

  /**
   * Follow successors for the given set.
   *
   * - If a hash does not have successors in this `dag`, then this hash
   *   will be included in the result.
   * - If a hash has multiple successors, only the "head" successor that
   *   is also in this `dag` will be returned, the hash itself will be
   *   excluded from the result.
   * - If `set` contains a hash that gets split into multiple successors
   *   that heads(succesors) on the mutation graph still contains multiple
   *   commits, then heads(ancestors(successors)) on the main graph will
   *   be attempted to pick the "stack top".
   *
   * For example, consider the successor relations:
   *
   *    A-->A1-->A2-->A3
   *
   * and if the current graph only has 'A1', 'A2' and 'B'.
   * followSuccessors(['A', 'B']) will return ['A2', 'B'].
   * successors(['A', 'B']) will return ['A', 'A1', 'A2', 'B'].
   */
  followSuccessors(set: SetLike): HashSet {
    const hashSet = HashSet.fromHashes(set);
    const mDag = this.mutationDag;
    let successors = mDag.heads(mDag.range(hashSet, this));
    // When following a split to multiple successors, consider using
    // the main dag to pick the stack top.
    if (hashSet.size === 1 && successors.size > 1) {
      successors = this.heads(this.ancestors(successors));
    }
    const obsoleted = mDag.ancestors(mDag.parents(successors));
    return hashSet.subtract(obsoleted).union(successors);
  }

  /** Attempt to resolve a name by `name`. The `name` can be a hash, a bookmark name, etc. */
  resolve(name: string): Readonly<Info> | undefined {
    // See `hg help revision` and context.py (changectx.__init__),
    // namespaces.py for priorities. Basically (in this order):
    // - hex full hash (40 bytes); '.' (working parent)
    // - nameMap (see infoToNameMapEntries)
    // - partial match (unambigious partial prefix match)

    // Full commit hash?
    const info = this.get(name);
    if (info) {
      return info;
    }

    // Namemap lookup.
    const entries = this.inner.nameMap.get(name);
    if (entries) {
      let best: HashPriRecord | null = null;
      for (const entry of entries) {
        if (best == null || best.priority > entry.priority) {
          best = entry;
        }
      }
      if (best != null) {
        return this.get(best.hash);
      }
    }

    // Unambigious prefix match.
    if (shouldPrefixMatch(name)) {
      let matched: undefined | Hash = undefined;
      for (const hash of this) {
        if (hash.startsWith(name)) {
          if (matched === undefined) {
            matched = hash;
          } else {
            // Ambigious prefix.
            return undefined;
          }
        }
      }
      return matched !== undefined ? this.get(matched) : undefined;
    }

    // No match.
    return undefined;
  }
}

type NameMapEntry = [string, HashPriRecord];

/** Extract the (name, hash, pri) infomration for insertion and deletion. */
function infoToNameMapEntries(info: Info): Array<NameMapEntry> {
  // Priority, highest to lowest:
  // - full hash (handled by dag.resolve())
  // - ".", the working parent
  // - namespaces.singlenode lookup
  //   - 10: bookmarks
  //   - 55: remotebookmarks (ex. "remote/main")
  //   - 60: hoistednames (ex. "main" without "remote/")
  //   - 70: phrevset (ex. "Dxxx"), but we skip it here due to lack
  //         of access to the code review abstraction.
  // - partial hash (handled by dag.resolve())
  const result: Array<NameMapEntry> = [];
  const {hash, isHead, bookmarks, remoteBookmarks} = info;
  if (isHead) {
    result.push(['.', HashPriRecord({hash, priority: 1})]);
  }
  bookmarks.forEach(b => result.push([b, HashPriRecord({hash, priority: 10})]));
  remoteBookmarks.forEach(rb => {
    result.push([rb, HashPriRecord({hash, priority: 55})]);
    const split = splitOnce(rb, '/')?.[1];
    if (split) {
      result.push([split, HashPriRecord({hash, priority: 60})]);
    }
  });
  return result;
}

/** Return the new NameMap after inserting or deleting `infos`. */
function calculateNewNameMap(
  map: NameMap,
  deleteInfos: Iterable<Readonly<Info>>,
  insertInfos: Iterable<Readonly<Info>>,
): NameMap {
  return map.withMutations(mut => {
    let map = mut;
    for (const info of deleteInfos) {
      const entries = infoToNameMapEntries(info);
      for (const [name, hashPri] of entries) {
        map = map.removeIn([name, hashPri]);
        if (map.get(name)?.isEmpty()) {
          map = map.remove(name);
        }
      }
    }
    for (const info of insertInfos) {
      const entries = infoToNameMapEntries(info);
      for (const [name, hashPri] of entries) {
        const set = map.get(name);
        if (set === undefined) {
          map = map.set(name, ImSet<HashPriRecord>([hashPri]));
        } else {
          map = map.set(name, set.add(hashPri));
        }
      }
    }
    return map;
  });
}

/** Decide whether `hash` looks like a hash prefix. */
function shouldPrefixMatch(hash: Hash): boolean {
  // No prefix match for full hashes.
  if (hash.length >= 40) {
    return false;
  }
  // No prefix match for non-hex hashes.
  return /^[0-9a-f]+$/.test(hash);
}

/** Distance ancestors that are treated as direct parents. */
type DagExt = {
  ancestors?: Hash[];
};

type Info = CommitInfo & WithPreviewType & DagExt;
type NameMap = ImMap<string, ImSet<HashPriRecord>>;

type CommitDagProps = {
  commitDag: BaseDag<Info>;
  mutationDag: MutationDag;
  // derived from Info, for fast "resolve" lookup. name -> hashpri
  nameMap: NameMap;
  nextSeqNumber: number;
};

const CommitDagRecord = Record<CommitDagProps>({
  commitDag: new BaseDag(),
  mutationDag: new MutationDag(),
  nameMap: ImMap() as NameMap,
  nextSeqNumber: 0,
});

type CommitDagRecord = RecordOf<CommitDagProps>;

type HashPriProps = {
  hash: Hash;
  // for 'resolve' use-case; lower number = higher priority
  priority: number;
};
const HashPriRecord = Record<HashPriProps>({hash: '', priority: 0});
type HashPriRecord = RecordOf<HashPriProps>;

const EMPTY_DAG_RECORD = CommitDagRecord();

/** 'Hash' prefix for rebase successor in preview. */
export const REBASE_SUCC_PREFIX = 'OPTIMISTIC_REBASE_SUCC:';

/** Default 'compare' function for sortAsc. */
const sortAscCompare = (a: Info, b: Info) => {
  // Consider phase. Public first.
  if (a.phase !== b.phase) {
    return a.phase === 'public' ? -1 : 1;
  }
  // Consider seqNumber (insertion order during preview calculation).
  if (a.seqNumber != null && b.seqNumber != null) {
    const seqDelta = a.seqNumber - b.seqNumber;
    if (seqDelta !== 0) {
      return seqDelta;
    }
  }
  // Sort by date.
  const timeDelta = b.date.getTime() - a.date.getTime();
  if (timeDelta !== 0) {
    return timeDelta;
  }
  // Always break ties even if timestamp is the same.
  return a.hash < b.hash ? 1 : -1;
};
