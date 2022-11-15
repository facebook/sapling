/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {DiffSide} from './generated/graphql';
import lineToPosition from './lineToPosition';

/**
 * Left (lines)
 * -----
 * 1 a
 * 2 common
 * 3 b
 *
 * Right (lines)
 * -----
 * 1 c
 * 2 common
 * 3 d
 *
 * Diff (positions)
 * -----
 * 0 @@ -1,3 +1,3 @@
 * 1 -a
 * 2 +c
 * 3 common
 * 4 -b
 * 5 +d
 */
test('one hunk', () => {
  const left = `a
common
b
`;
  const right = `c
common
d
`;

  const mapping = lineToPosition(left, right);
  const leftMapping = mapping[DiffSide.Left];
  const rightMapping = mapping[DiffSide.Right];

  expect(leftMapping).toEqual({
    1: 1,
    2: 3,
    3: 4,
  });
  expect(rightMapping).toEqual({
    1: 2,
    2: 3,
    3: 5,
  });
});

/**
 * Left (lines)
 * -----
 * 1 a
 * 2 common
 * 3 common
 * 4 common
 * 5 common
 * 6 common
 * 7 common
 * 8 common
 * 9 b
 *
 * Right (lines)
 * -----
 * 1 c
 * 2 common
 * 3 common
 * 4 common
 * 5 common
 * 6 common
 * 7 common
 * 8 common
 * 9 d
 *
 * Diff (positions)
 * -----
 *  0 @@ -1,4 +1,4 @@
 *  1 -a
 *  2 +c
 *  3 common
 *  4 common
 *  5 common
 *  6 @@ -6,4 +6,4 @@
 *  7 common
 *  8 common
 *  9 common
 * 10 -b
 * 11 +d
 */
test('multiple hunks', () => {
  const left = `a
common
common
common
common
common
common
common
b
`;
  const right = `c
common
common
common
common
common
common
common
d
`;

  const mapping = lineToPosition(left, right);
  const leftMapping = mapping[DiffSide.Left];
  const rightMapping = mapping[DiffSide.Right];

  expect(leftMapping).toEqual({
    1: 1,
    2: 3,
    3: 4,
    4: 5,
    6: 7,
    7: 8,
    8: 9,
    9: 10,
  });
  expect(rightMapping).toEqual({
    1: 2,
    2: 3,
    3: 4,
    4: 5,
    6: 7,
    7: 8,
    8: 9,
    9: 11,
  });
});

/**
 * Left (lines)
 * -----
 * 1 a
 *
 * Right (lines)
 * -----
 * 1 d
 * 2 e
 * 3 f
 *
 * Diff (positions)
 * -----
 * 0 @@ -1,1 +1,3 @@
 * 1 -a
 * 2 +d
 * 3 +e
 * 4 +f
 */
test('multi-line addition', () => {
  const left = `a`;
  const right = `d
e
f
`;

  const mapping = lineToPosition(left, right);
  const leftMapping = mapping[DiffSide.Left];
  const rightMapping = mapping[DiffSide.Right];

  expect(leftMapping).toEqual({1: 1});
  expect(rightMapping).toEqual({
    1: 2,
    2: 3,
    3: 4,
  });
});

/**
 * Left (lines)
 * -----
 * 1 a
 * 2 b
 * 3 c
 *
 * Right (lines)
 * -----
 * 1 d
 *
 * Diff (positions)
 * -----
 * 0 @@ -1,3 +1,1 @@
 * 1 -a
 * 2 -b
 * 3 -c
 * 4 +d
 */
test('multi-line removal', () => {
  const left = `a
b
c
`;
  const right = `d`;

  const mapping = lineToPosition(left, right);
  const leftMapping = mapping[DiffSide.Left];
  const rightMapping = mapping[DiffSide.Right];

  expect(leftMapping).toEqual({
    1: 1,
    2: 2,
    3: 3,
  });
  expect(rightMapping).toEqual({1: 4});
});

/**
 * Left (lines)
 * -----
 * 1 a
 * 2 b
 * 3 c
 *
 * Right (lines)
 * -----
 * 1 d
 * 2 e
 * 3 f
 *
 * Diff (positions)
 * -----
 * 0 @@ -1,3 +1,3 @@
 * 1 -a
 * 2 -b
 * 3 -c
 * 4 +d
 * 5 +e
 * 6 +f
 */
test('multi-line group', () => {
  const left = `a
b
c
`;
  const right = `d
e
f
`;

  const mapping = lineToPosition(left, right);
  const leftMapping = mapping[DiffSide.Left];
  const rightMapping = mapping[DiffSide.Right];

  expect(leftMapping).toEqual({
    1: 1,
    2: 2,
    3: 3,
  });
  expect(rightMapping).toEqual({
    1: 4,
    2: 5,
    3: 6,
  });
});
