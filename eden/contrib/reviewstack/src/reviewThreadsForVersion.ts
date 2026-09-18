/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {GitHubPullRequestReviewThread} from './github/pullRequestTimelineTypes';
import type {GitObjectID, Version} from './github/types';
import type {ThreadsBySide} from './jotai/atoms';

export default function reviewThreadsForVersion(
  allThreads: GitHubPullRequestReviewThread[],
  versions: Version[],
  commitID: GitObjectID,
  path: string,
): ThreadsBySide | null {
  const versionByCommit = new Map<GitObjectID, number>();
  versions.forEach(({commits}, versionIndex) => {
    commits.forEach(({commit}) => versionByCommit.set(commit, versionIndex));
  });
  const targetVersion = versionByCommit.get(commitID);
  if (targetVersion == null) {
    return null;
  }

  const result: ThreadsBySide = {LEFT: [], RIGHT: []};
  allThreads.forEach(thread => {
    const firstComment = thread.comments[0];
    const sourceCommit = firstComment?.originalCommit?.oid ?? firstComment?.commit?.oid;
    const sourceVersion = sourceCommit == null ? null : versionByCommit.get(sourceCommit);
    if (firstComment?.path !== path || sourceVersion == null || sourceVersion > targetVersion) {
      return;
    }

    const annotatedThread = {
      ...thread,
      sourceVersionIndex: sourceVersion,
      isHistorical: sourceVersion < targetVersion,
    };
    if (sourceVersion < targetVersion) {
      result.RIGHT.push(annotatedThread);
    } else {
      result[thread.diffSide].push(annotatedThread);
    }
  });
  return result;
}
