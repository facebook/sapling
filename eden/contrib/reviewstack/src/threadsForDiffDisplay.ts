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
  oldLines: ReadonlySet<number> = new Set(),
  oldCommitID?: string,
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
  const attachToPreviousVersionLine = (thread: GitHubPullRequestReviewThread) =>
    thread.sourceVersionIndex != null &&
    thread.targetVersionIndex === thread.sourceVersionIndex + 1 &&
    thread.sourceVersionHeadCommit === oldCommitID &&
    thread.originalLine != null &&
    oldLines.has(thread.originalLine);
  const currentByLine = (side: DiffSide) =>
    groupBy(
      allThreads[side].filter(thread => thread.isHistorical !== true),
      thread => thread.originalLine ?? null,
    );

  const before = currentByLine(DiffSide.Left);
  historical.filter(attachToPreviousVersionLine).forEach(thread => {
    const line = thread.originalLine;
    if (line != null) {
      before.set(line, [...(before.get(line) ?? []), thread]);
    }
  });

  return {
    historical: historical.filter(thread => !attachToPreviousVersionLine(thread)),
    before,
    after: currentByLine(DiffSide.Right),
  };
}
