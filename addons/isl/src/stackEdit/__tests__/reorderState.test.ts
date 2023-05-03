/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Rev} from '../fileStackState';

import {reorderWithDeps} from '../reorderState';

describe('reorderWithDeps', () => {
  const depMap = new Map<Rev, Set<Rev>>([
    [3, new Set([2])],
    [4, new Set([2])],
    [5, new Set([3, 1])],
  ]);

  it('moves nothing if offset is 0', () => {
    expect(reorderWithDeps(5, 3, 0, depMap)).toMatchObject({
      order: [0, 1, 2, 3, 4],
      deps: [3],
    });
  });

  it('moves down without deps', () => {
    expect(reorderWithDeps(5, 4, -1, depMap)).toMatchObject({
      order: [0, 1, 2, 4, 3],
      deps: [4],
    });
  });

  it('moves up without deps', () => {
    expect(reorderWithDeps(5, 0, 1, depMap)).toMatchObject({
      order: [1, 0, 2, 3, 4],
      deps: [0],
    });

    expect(reorderWithDeps(5, 0, 4, depMap)).toMatchObject({
      order: [1, 2, 3, 4, 0],
      deps: [0],
    });
  });

  it('bounds out of range offsets', () => {
    expect(reorderWithDeps(5, 3, 999, new Map())).toMatchObject({
      order: [0, 1, 2, 4, 3],
      deps: [3],
    });

    expect(reorderWithDeps(5, 3, -999, new Map())).toMatchObject({
      order: [3, 0, 1, 2, 4],
      deps: [3],
    });
  });

  it('moves down with deps', () => {
    // Move 4 to before 2, [4, 2] changed to [2, 4] for deps.
    expect(reorderWithDeps(5, 4, -2, depMap)).toMatchObject({
      order: [0, 1, 2, 4, 3],
      deps: [2, 4],
    });

    // Move 4 to before 1, [2, 4] are moved together.
    expect(reorderWithDeps(5, 4, -3, depMap)).toMatchObject({
      order: [0, 2, 4, 1, 3],
      deps: [2, 4],
    });

    // Move 5 to the bottom. 5->3, 5->1, 3->2 deps are considered.
    expect(reorderWithDeps(6, 5, -5, depMap)).toMatchObject({
      order: [1, 2, 3, 5, 0, 4],
      deps: [1, 2, 3, 5],
    });
  });

  it('moves up with deps', () => {
    // Moves 1 up and 1->5 dep is considered.
    expect(reorderWithDeps(6, 1, 4, depMap)).toMatchObject({
      order: [0, 2, 3, 4, 1, 5],
      deps: [1, 5],
    });

    // Moves 2 up and 2->3, 2->4, 3->5 deps are considered.
    expect(reorderWithDeps(6, 2, 3, depMap)).toMatchObject({
      order: [0, 1, 2, 3, 4, 5],
      deps: [2, 3, 4, 5],
    });
  });
});
