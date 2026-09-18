/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import sumFileLineChanges from './sumFileLineChanges';

test('sums line changes for the active commit comparison', () => {
  expect(
    sumFileLineChanges([
      {additions: 12, deletions: 4},
      {additions: 7, deletions: 9},
    ]),
  ).toEqual({additions: 19, deletions: 13});
});
