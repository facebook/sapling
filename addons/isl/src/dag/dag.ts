/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {WithPreviewType} from '../previews';
import type {CommitInfo, Hash} from '../types';
import type {SetLike} from './set';
import type {RecordOf} from 'immutable';

import {CommitPreview} from '../previews';
import {HashSet} from './set';
import {Map as ImMap, Record, List} from 'immutable';
import {cached} from 'shared/LRU';
import {SelfUpdate} from 'shared/immutableExt';
import {splitOnce, unwrap} from 'shared/utils';

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
  remove(set: SetLike): Dag<C> {
    const hashSet = HashSet.fromHashes(set);
    let {childMap, infoMap} = this;
    for (const hash of hashSet) {
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
    set: SetLike,
    replaceFunc: (hash: Hash, commit: C | undefined) => C | undefined,
  ): Dag<C> {
    const hashSet = HashSet.fromHashes(set);
    const hashes = hashSet.toHashes();
    return this.remove(hashSet).add(
      hashes.map(h => replaceFunc(h, this.get(h))).filter(c => c != undefined) as Iterable<C>,
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

  // High-level query

  parents(set: SetLike): HashSet {
    return flatMap(set, h => this.parentHashes(h));
  }

  children(set: SetLike): HashSet {
    return flatMap(set, h => this.childHashes(h));
  }

  /**
   * set + parents(set) + parents(parents(set)) + ...
   * If `within` is set, change `parents` to only return hashes within `within`.
   */
  @cached({cacheSize: 500})
  ancestors(set: SetLike, props?: {within?: SetLike}): HashSet {
    const filter = nullableWithinContains(props?.within);
    return unionFlatMap(set, h => this.parentHashes(h).filter(filter));
  }

  /**
   * set + children(set) + children(children(set)) + ...
   * If `within` is set, change `children` to only return hashes within `within`.
   */
  descendants(set: SetLike, props?: {within?: SetLike}): HashSet {
    const filter = nullableWithinContains(props?.within);
    return unionFlatMap(set, h => this.childHashes(h).filter(filter));
  }

  /** ancestors(heads) & descendants(roots) */
  range(roots: SetLike, heads: SetLike): HashSet {
    // PERF: This is not the most efficient, but easy to write.
    return this.ancestors(heads).intersect(this.descendants(roots));
  }

  /** set - children(set) */
  roots(set: SetLike): HashSet {
    const children = this.children(set);
    return HashSet.fromHashes(set).subtract(children);
  }

  /** set - parents(set) */
  heads(set: SetLike): HashSet {
    const parents = this.parents(set);
    return HashSet.fromHashes(set).subtract(parents);
  }

  /** Greatest common ancestor. heads(ancestors(set1) & ancestors(set2)). */
  gca(set1: SetLike, set2: SetLike): HashSet {
    return this.heads(this.ancestors(set1).intersect(this.ancestors(set2)));
  }

  /** ancestor in ancestors(descendant) */
  isAncestor(ancestor: Hash, descendant: Hash): boolean {
    // PERF: This is not the most efficient, but easy to write.
    return this.ancestors(descendant).contains(ancestor);
  }

  /**
   * Return commits that match the given condition.
   * This can be useful for things like "obsolete()".
   * `set`, if not undefined, limits the search space.
   */
  filter(predicate: (commit: Readonly<C>) => boolean, set?: SetLike): HashSet {
    let hashes: SetLike;
    if (set === undefined) {
      hashes = this.infoMap.filter((commit, _hash) => predicate(commit)).keys();
    } else {
      hashes = HashSet.fromHashes(set)
        .toHashes()
        .filter(h => {
          const c = this.get(h);
          return c != undefined && predicate(c);
        });
    }
    return HashSet.fromHashes(hashes);
  }

  // Delegates

  get infoMap(): ImMap<Hash, Readonly<C>> {
    return this.inner.infoMap;
  }

  get childMap(): ImMap<Hash, List<Hash>> {
    return this.inner.childMap;
  }

  // Filters. Some of them are less generic, require `C` to be `CommitInfo`.

  obsolete(set?: SetLike): HashSet {
    return this.filter(c => (c as Partial<CommitInfo>).successorInfo != null, set);
  }

  public_(set?: SetLike): HashSet {
    return this.filter(c => (c as Partial<CommitInfo>).phase === 'public', set);
  }

  draft(set?: SetLike): HashSet {
    return this.filter(c => ((c as Partial<CommitInfo>).phase ?? 'draft') === 'draft', set);
  }

  merge(set?: SetLike): HashSet {
    return this.filter(c => c.parents.length > 1, set);
  }

  // Edit APIs that are less generic, require `C` to be `CommitInfo`.

  /** Bump the timestamp of descendants(set) to "now". */
  touch(set: SetLike, includeDescendants = true): Dag<C> {
    const affected = includeDescendants ? this.descendants(set) : set;
    return this.replaceWith(affected, (_h, c) => {
      if (c && (c as Partial<CommitInfo>).date) {
        return {...c, date: new Date()};
      } else {
        return c;
      }
    });
  }

  /// Remove obsoleted commits that no longer have non-obsoleted descendants.
  cleanup(): Dag<C> {
    // ancestors(".") are not obsoleted.
    const obsolete = this.obsolete().subtract(this.ancestors(this.resolve('.')?.hash));
    const heads = this.heads(this.draft()).intersect(obsolete);
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
  rebase(srcSet: SetLike, dest: Hash | undefined): Dag<C> {
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
    return this.replaceWith(src.union(duplicated.toHashes().map(maybeSuccHash)), (h, c) => {
      const isSucc = h.startsWith(REBASE_SUCC_PREFIX);
      const pureHash = isSucc ? h.substring(REBASE_SUCC_PREFIX.length) : h;
      const isPred = !isSucc && duplicated.contains(h);
      const isRoot = srcRoots.contains(pureHash);
      const info = unwrap(isSucc ? this.get(pureHash) : c);
      const newInfo: Partial<CommitInfo & WithPreviewType> = {};
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
    }).cleanup();
  }

  // Query APIs that are less generic, require `C` to be `CommitInfo`.

  /// All successors recursively.
  successors(set: SetLike): HashSet {
    const getSuccessors = (h: Hash) => {
      const info: Partial<CommitInfo> | undefined = this.get(h);
      const succ = info?.successorInfo?.hash;
      return succ == null ? [] : [succ];
    };
    return unionFlatMap(set, getSuccessors);
  }

  /** Attempt to resolve a name by `name`. The `name` can be a hash, a bookmark name, etc. */
  resolve(name: string): Readonly<C> | undefined {
    // Full commit hash?
    const info = this.get(name);
    if (info) {
      return info;
    }
    // Scan through the commits.
    // See `hg help revision` and context.py (changectx.__init__),
    // namespaces.py for priorities. Basically (in this order):
    // - ".", the working parent
    // - hex full hash (40 bytes) (handled above)
    // - namespaces.singlenode lookup
    //   - 10: bookmarks
    //   - 55: remotebookmarks (ex. "remote/main")
    //   - 60: hoistednames (ex. "main" without "remote/")
    //   - 70: phrevset (ex. "Dxxx"), but we skip it here due to lack
    //         of access to the code review abstraction.
    // - partial match (unambigious partial prefix match)
    type Best = {hash: Hash; priority: number; info: Partial<CommitInfo>};
    const best: {value?: Best} = {};
    for (const [hash, commit] of this.infoMap) {
      const info = commit as Partial<CommitInfo>;
      const updateBest = (priority: number) => {
        if (
          best.value == null ||
          best.value.priority > priority ||
          (best.value.info.date ?? 0) < (info.date ?? 0) ||
          best.value.hash < hash
        ) {
          best.value = {hash, priority, info} as Best;
        }
      };
      if (name === '.' && info.isHead) {
        updateBest(1);
      } else if ((info.bookmarks ?? []).includes(name)) {
        updateBest(10);
      } else if ((info.remoteBookmarks ?? []).includes(name)) {
        updateBest(55);
      } else if ((info.remoteBookmarks ?? []).map(n => splitOnce(n, '/')?.[1]).includes(name)) {
        updateBest(60);
      }
    }
    const hash = best.value?.hash;
    if (hash != null) {
      return this.get(hash);
    }
    // Unambigious prefix match.
    let matched: undefined | Hash = undefined;
    for (const hash of this.infoMap.keys()) {
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
}

function flatMap(set: SetLike, f: (h: Hash) => List<Hash> | Readonly<Array<Hash>>): HashSet {
  return new HashSet(
    HashSet.fromHashes(set)
      .toHashes()
      .flatMap(h => f(h)),
  );
}

/** set + flatMap(set, f) + flatMap(flatMap(set, f), f) + ... */
function unionFlatMap(set: SetLike, f: (h: Hash) => List<Hash> | Readonly<Array<Hash>>): HashSet {
  let result = new HashSet().toHashes();
  let newHashes = [...HashSet.fromHashes(set)];
  while (newHashes.length > 0) {
    result = result.concat(newHashes);
    const nextNewHashes: Hash[] = [];
    newHashes.forEach(h => {
      f(h).forEach(v => {
        if (!result.contains(v)) {
          nextNewHashes.push(v);
        }
      });
    });
    newHashes = nextNewHashes;
  }
  return HashSet.fromHashes(result);
}

/**
 * If `set` is undefined, return a function that always returns true.
 * Otherwise, return a function that checks whether `set` contains `h`.
 */
function nullableWithinContains(set?: SetLike): (h: Hash) => boolean {
  if (set === undefined) {
    return _h => true;
  } else {
    const hashSet = HashSet.fromHashes(set);
    return h => hashSet.contains(h);
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

/** 'Hash' prefix for rebase successor in preview. */
export const REBASE_SUCC_PREFIX = 'OPTIMISTIC_REBASE_SUCC:';
