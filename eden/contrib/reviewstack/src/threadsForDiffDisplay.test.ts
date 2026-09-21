/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {GitHubPullRequestReviewThread} from './github/pullRequestTimelineTypes';

import {DiffSide} from './generated/graphql';
import threadsForDiffDisplay from './threadsForDiffDisplay';

function thread(id: string, originalLine: number, isHistorical = false) {
  return {id, originalLine, isHistorical} as GitHubPullRequestReviewThread;
}

test('keeps historical threads visible without attaching them to obsolete lines', () => {
  const historical = thread('historical', 17, true);
  const current = thread('current', 23);
  const result = threadsForDiffDisplay({
    [DiffSide.Left]: [historical],
    [DiffSide.Right]: [historical, current],
  });

  expect(result.historical).toEqual([historical]);
  expect(result.before.size).toBe(0);
  expect(result.after.get(17)).toBeUndefined();
  expect(result.after.get(23)).toEqual([current]);
});

test('anchors a previous-version comment to its line when that line is in the new diff', () => {
  const previous = {
    ...thread('previous', 17, true),
    sourceVersionIndex: 4,
    targetVersionIndex: 5,
    sourceVersionHeadCommit: 'v5',
  };
  const older = {
    ...thread('older', 18, true),
    sourceVersionIndex: 2,
    targetVersionIndex: 5,
  };
  const result = threadsForDiffDisplay(
    {[DiffSide.Left]: [], [DiffSide.Right]: [previous, older]},
    new Set([17, 18]),
    'v5',
  );

  expect(result.before.get(17)).toEqual([previous]);
  expect(result.before.get(18)).toBeUndefined();
  expect(result.historical).toEqual([older]);
});
