/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import Immutable, {List} from 'immutable';
import {SelfUpdate} from '../immutableExt';

describe('SelfUpdate', () => {
  it('is needed because of immutable.js deepEquals', () => {
    const list1 = nestedList(10);
    const list2 = nestedList(10);
    // Immutable.is performs deepEqual repetitively.
    expect(immutableIsCallCounts(list1, list2)).toMatchObject([11, 11, 11]);
  });

  it('avoids repetitive deepEquals', () => {
    const list1 = new SelfUpdate(nestedList(10));
    const list2 = new SelfUpdate(nestedList(10));
    expect(immutableIsCallCounts(list1, list2)).toMatchObject([11, 1, 1]);
  });

  it('does not equal to a different type', () => {
    const list1 = new SelfUpdate(nestedList(10));
    const list2 = nestedList(10);
    expect(Immutable.is(list1, list2)).toBeFalsy();
    expect(Immutable.is(list2, list1)).toBeFalsy();
    expect(list2.equals(list1)).toBeFalsy();
    expect(list1.equals(list2)).toBeFalsy();
  });

  it('helps when used as a nested item', () => {
    const list1 = List([List([new SelfUpdate(nestedList(8))])]);
    const list2 = List([List([new SelfUpdate(nestedList(8))])]);
    expect(immutableIsCallCounts(list1, list2)).toMatchObject([11, 3, 3]);
  });
});

type NestedList = List<number | NestedList>;

/** Construct a nested List of a given depth. */
function nestedList(depth: number): NestedList {
  return depth <= 0 ? List([10]) : List([nestedList(depth - 1)]);
}

/** Call Immutable.is n times, return call counts. */
function immutableIsCallCounts(a: unknown, b: unknown, n = 3): Array<number> {
  const ListEqualsMock = jest.spyOn(List.prototype, 'equals');
  const counts = Array.from({length: n}, () => {
    if (!Immutable.is(a, b)) {
      return -1;
    }
    const count = ListEqualsMock.mock.calls.length;
    ListEqualsMock.mockClear();
    return count;
  });
  ListEqualsMock.mockRestore();
  return counts;
}
