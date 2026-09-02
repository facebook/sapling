/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitRev} from '../commitStackState';

export type DragDestinationRow = {
  midpoint: number;
  rev: CommitRev;
  top: number;
};

export function findDragDestinationCommitRev(
  y: number,
  rows: ReadonlyArray<DragDestinationRow>,
): CommitRev | undefined {
  let bestRow: DragDestinationRow | undefined;
  let bestDistance = Infinity;
  for (const row of rows) {
    if (row.top > y) {
      continue;
    }
    const distance = Math.abs(y - row.midpoint);
    if (distance < bestDistance) {
      bestRow = row;
      bestDistance = distance;
    }
  }
  return bestRow?.rev;
}
