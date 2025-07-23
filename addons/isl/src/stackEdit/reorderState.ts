/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitRev} from './commitStackState';

import {List, Record} from 'immutable';
import {CommitStackState} from './commitStackState';

type ReorderResult = {
  /** Offset of the move. Positive: move up. Negative: move down. */
  offset: number;

  /** Reorder result that satisfy dependencies. */
  order: CommitRev[];

  /** Dependent revs that are also moved. */
  deps: CommitRev[];
};

function* range(
  start: number,
  end: number,
  filterFunc?: (i: CommitRev) => boolean,
): IterableIterator<CommitRev> {
  for (let i = start; i < end; i++) {
    if (filterFunc && !filterFunc(i as CommitRev)) {
      continue;
    }
    yield i as CommitRev;
  }
}

/**
 * Reorder 0..n (exclusive) by moving `origRev` by `offset`.
 * Respect `depMap`.
 */
export function reorderWithDeps(
  n: number,
  origRev: CommitRev,
  desiredOffset: number,
  depMap: Readonly<Map<CommitRev, Set<CommitRev>>>,
): Readonly<ReorderResult> {
  const offset =
    origRev + desiredOffset < 0
      ? -origRev
      : origRev + desiredOffset >= n
        ? n - 1 - origRev
        : desiredOffset;

  let order: CommitRev[] = [];
  const deps: CommitRev[] = [origRev];
  const filterFunc = (i: CommitRev) => !deps.includes(i);
  if (offset < 0) {
    // Moved down.
    const depRevs = new Set(depMap.get(origRev) ?? []);
    for (let i = -1; i >= offset; i--) {
      const rev = (origRev + i) as CommitRev;
      if (depRevs.has(rev)) {
        deps.push(rev);
        depMap.get(rev)?.forEach(r => depRevs.add(r));
      }
    }
    deps.reverse();
    order = [...range(0, origRev + offset), ...deps, ...range(origRev + offset, n, filterFunc)];
  } else if (offset > 0) {
    // Moved up.
    for (let i = 1; i <= offset; i++) {
      const rev = (origRev + i) as CommitRev;
      const dep = depMap.get(rev);
      if (dep && (dep.has(origRev) || deps.some(r => dep.has(r)))) {
        deps.push(rev);
      }
    }
    order = [
      ...range(0, origRev + offset + 1, filterFunc),
      ...deps,
      ...range(origRev + offset + 1, n, filterFunc),
    ];
  } else {
    // Nothing moved.
    order = [...range(0, n)];
  }
  return {offset, order, deps};
}

/** State to preview effects of drag-n-drop reorder. */
export class ReorderState extends Record({
  offset: 0,
  commitStack: new CommitStackState([]),
  reorderRevs: List<CommitRev>(),
  draggingRevs: List<CommitRev>(),
  draggingRev: -1 as CommitRev,
}) {
  static init(commitStack: CommitStackState, draggingRev: CommitRev): ReorderState {
    return new ReorderState({
      offset: 0,
      commitStack,
      draggingRev,
      reorderRevs: List(commitStack.revs()),
      draggingRevs: List([draggingRev]),
    });
  }

  isDragging() {
    return this.draggingRev >= 0;
  }

  /** Returns true if the reorder does nothing. */
  isNoop(): boolean {
    return this.offset === 0;
  }

  /**
   * Calculate reorderRevs and draggingRevs based on the given offset.
   * `draggingRevs` might change to maintain the dependency map.
   */
  withOffset(offset: number): ReorderState {
    const reordered = reorderWithDeps(
      this.commitStack.stack.size,
      this.draggingRev,
      offset,
      this.commitStack.calculateDepMap(),
    );

    // Force match dependency requirements of `rev` by moving dependencies.
    return this.merge({
      reorderRevs: List(reordered.order),
      draggingRevs: List(reordered.deps),
      offset: reordered.offset,
    });
  }
}
