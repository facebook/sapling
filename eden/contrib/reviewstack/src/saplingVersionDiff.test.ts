/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import saplingVersionDiffPairs from './saplingVersionDiff';

test('isolates rebased Sapling versions from their respective parents', () => {
  expect(
    saplingVersionDiffPairs(
      {oid: 'version-4', parents: ['parent-version-4']},
      {oid: 'version-5', parents: ['parent-version-5']},
    ),
  ).toEqual({
    before: {baseCommitID: 'parent-version-4', commitID: 'version-4'},
    after: {baseCommitID: 'parent-version-5', commitID: 'version-5'},
  });
});

test('uses the selected version parent when no earlier version is selected', () => {
  expect(saplingVersionDiffPairs(null, {oid: 'version-5', parents: ['parent-version-5']})).toEqual({
    before: null,
    after: {baseCommitID: 'parent-version-5', commitID: 'version-5'},
  });
});

test('does not isolate merge commits', () => {
  expect(
    saplingVersionDiffPairs(
      {oid: 'version-4', parents: ['parent-version-4']},
      {oid: 'version-5', parents: ['parent-a', 'parent-b']},
    ),
  ).toBeNull();
});
