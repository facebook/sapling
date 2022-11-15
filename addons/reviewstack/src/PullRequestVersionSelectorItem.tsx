/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DateTime, GitObjectID, VersionCommit} from './github/types';

import BulletItems from './BulletItems';
import CommentCount from './CommentCount';
import CommitLink from './CommitLink';
import {
  gitHubPullRequestComparableVersions,
  gitHubPullRequestThreadsByCommit,
  gitHubPullRequestSelectedVersionIndex,
} from './recoil';
import {countCommentsForThreads, formatISODate, versionLabel} from './utils';
import {ActionList, Box, Text} from '@primer/react';
import React, {useCallback} from 'react';
import {useRecoilState, useRecoilValue, useSetRecoilState} from 'recoil';

// eslint-disable-next-line prefer-arrow-callback
export default React.memo(function PullRequestVersionSelectorItem({
  baseParent = null,
  commit,
  commits,
  committedDate,
  index,
  org,
  repo,
}: {
  baseParent?: GitObjectID | null;
  commit: GitObjectID;
  commits: VersionCommit[];
  committedDate: DateTime;
  index: number;
  org: string;
  repo: string;
}): React.ReactElement {
  const [selectedVersionIndex, setSelectedVersionIndex] = useRecoilState(
    gitHubPullRequestSelectedVersionIndex,
  );
  const setComparableVersions = useSetRecoilState(gitHubPullRequestComparableVersions);
  const reviewThreadsByCommit = useRecoilValue(gitHubPullRequestThreadsByCommit);
  const commentCount = commits.reduce((acc, commit) => {
    const reviewThreadsForCommit = reviewThreadsByCommit.get(commit.commit);
    if (reviewThreadsForCommit == null) {
      return acc;
    }
    return acc + countCommentsForThreads(reviewThreadsForCommit);
  }, 0);

  const onClick = useCallback(() => {
    setSelectedVersionIndex(index);
    setComparableVersions({
      beforeCommitID: null,
      afterCommitID: commit,
    });
  }, [commit, index, setComparableVersions, setSelectedVersionIndex]);

  const numCommits = commits.length;

  return (
    <ActionList.Item
      selected={selectedVersionIndex === index}
      onSelect={onClick}
      sx={{display: 'flex', alignItems: 'center'}}>
      <Box fontSize={0}>
        <Box display="flex" alignItems="center" gridGap={2}>
          <Text fontWeight="bold" fontSize={1}>
            {versionLabel(index)}
          </Text>{' '}
          <BulletItems>
            {formatISODate(committedDate)}
            <Box>
              {numCommits} commit{numCommits === 1 ? '' : 's'}
            </Box>
            {commentCount > 0 && <CommentCount count={commentCount} />}
          </BulletItems>
        </Box>
        <BulletItems>
          <Box>
            Head Commit: <CommitLink org={org} repo={repo} oid={commit} />
          </Box>
          <Box>
            Base Commit:{' '}
            {baseParent == null ? 'null' : <CommitLink org={org} repo={repo} oid={baseParent} />}
          </Box>
        </BulletItems>
      </Box>
    </ActionList.Item>
  );
});
