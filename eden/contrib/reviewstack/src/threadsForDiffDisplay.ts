/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  GitHubPullRequestReviewThread,
  GitHubPullRequestReviewThreadsByLine,
} from './github/pullRequestTimelineTypes';

import {DiffSide} from './generated/graphql';
import {groupBy} from './utils';

export default function threadsForDiffDisplay(
  allThreads: {[key in DiffSide]: GitHubPullRequestReviewThread[]} | null,
): {
  historical: GitHubPullRequestReviewThread[];
  before: GitHubPullRequestReviewThreadsByLine;
  after: GitHubPullRequestReviewThreadsByLine;
} {
  if (allThreads == null) {
    return {historical: [], before: new Map(), after: new Map()};
  }

  const historical = Array.from(
    new Map(
      [...allThreads[DiffSide.Left], ...allThreads[DiffSide.Right]]
        .filter(thread => thread.isHistorical === true)
        .map(thread => [thread.id, thread]),
    ).values(),
  );
  const currentByLine = (side: DiffSide) =>
    groupBy(
      allThreads[side].filter(thread => thread.isHistorical !== true),
      thread => thread.originalLine ?? null,
    );

  return {
    historical,
    before: currentByLine(DiffSide.Left),
    after: currentByLine(DiffSide.Right),
  };
}
