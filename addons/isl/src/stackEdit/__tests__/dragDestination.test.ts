/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitRev} from '../commitStackState';
import type {DragDestinationRow} from '../ui/dragDestination';

import {findDragDestinationCommitRev} from '../ui/dragDestination';

describe('findDragDestinationCommitRev', () => {
  const rows: ReadonlyArray<DragDestinationRow> = [
    {midpoint: 50, rev: 0 as CommitRev, top: 0},
    {midpoint: 150, rev: 1 as CommitRev, top: 100},
    {midpoint: 250, rev: 2 as CommitRev, top: 200},
  ];

  it('returns no destination above the first row', () => {
    expect(findDragDestinationCommitRev(-1, rows)).toBeUndefined();
  });

  it('selects the closest midpoint from rows whose top has been reached', () => {
    expect(findDragDestinationCommitRev(100, rows)).toBe(0);
    expect(findDragDestinationCommitRev(101, rows)).toBe(1);
  });

  it('selects the last row when below all rows', () => {
    expect(findDragDestinationCommitRev(1000, rows)).toBe(2);
  });

  it('considers non-adjacent rows when bounds overlap', () => {
    const overlappingRows: ReadonlyArray<DragDestinationRow> = [
      {midpoint: 500, rev: 0 as CommitRev, top: 0},
      {midpoint: 125, rev: 1 as CommitRev, top: 100},
      {midpoint: 225, rev: 2 as CommitRev, top: 200},
    ];

    expect(findDragDestinationCommitRev(490, overlappingRows)).toBe(0);
  });
});
