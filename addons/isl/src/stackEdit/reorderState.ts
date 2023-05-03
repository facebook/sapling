/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Rev} from './fileStackState';

type ReorderResult = {
  /** Reorder result that satisfy dependencies. */
  order: Rev[];

  /** Dependent revs that are also moved. */
  deps: Rev[];
};

function* range(
  start: number,
  end: number,
  filterFunc?: (i: number) => boolean,
): IterableIterator<number> {
  for (let i = start; i < end; i++) {
    if (filterFunc && !filterFunc(i)) {
      continue;
    }
    yield i;
  }
}

/**
 * Reorder 0..n (exclusive) by moving `origRev` by `offset`.
 * Respect `depMap`.
 */
export function reorderWithDeps(
  n: number,
  origRev: Rev,
  desiredOffset: number,
  depMap: Readonly<Map<Rev, Set<Rev>>>,
): Readonly<ReorderResult> {
  const offset =
    origRev + desiredOffset < 0
      ? -origRev
      : origRev + desiredOffset >= n
      ? n - 1 - origRev
      : desiredOffset;

  let order: Rev[] = [];
  const deps: Rev[] = [origRev];
  const filterFunc = (i: Rev) => deps.indexOf(i) < 0;
  if (offset < 0) {
    // Moved down.
    const depRevs = new Set(depMap.get(origRev) ?? []);
    for (let i = -1; i >= offset; i--) {
      const rev = origRev + i;
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
      const rev = origRev + i;
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
  return {order, deps};
}
