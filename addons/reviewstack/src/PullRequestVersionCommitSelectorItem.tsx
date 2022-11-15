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
import ToggleButton from './ToggleButton';
import {gitHubPullRequestComparableVersions, gitHubPullRequestThreadsForCommit} from './recoil';
import {countCommentsForThreads, formatISODate} from './utils';
import {ActionList, Box, Text} from '@primer/react';
import React, {useCallback} from 'react';
import {useRecoilValue, useSetRecoilState} from 'recoil';

const TOGGLE_BUTTON_WIDTH = 70;

// eslint-disable-next-line prefer-arrow-callback
export default React.memo(function PullRequestVersionCommitSelectorItem({
  beforeCommitID,
  afterCommitID,
  author,
  commit,
  commits,
  committedDate,
  index,
  maxIndex,
  message,
  beforeIndex,
  afterIndex,
  org,
  repo,
}: {
  beforeCommitID: GitObjectID | null;
  afterCommitID: GitObjectID;
  author: string | null;
  commit: GitObjectID;
  commits: VersionCommit[];
  committedDate: DateTime;
  index: number;
  maxIndex: number;
  message: string;
  beforeIndex: number;
  afterIndex: number;
  org: string;
  repo: string;
}): React.ReactElement {
  const setComparableVersions = useSetRecoilState(gitHubPullRequestComparableVersions);
  const reviewThreadsForCommit = useRecoilValue(gitHubPullRequestThreadsForCommit(commit));
  const commentCount = countCommentsForThreads(reviewThreadsForCommit);

  const onClickBefore = useCallback(() => {
    // If the currently selected "before" is clicked, unselect it to compare
    // "after" against base.
    const newBefore = beforeIndex === index ? null : commit;
    // If the currently selected "after" is already newer, then keep it.
    // Otherwise, update to the immediate next version.
    const newAfter = afterIndex > index ? afterCommitID : commits[index + 1]?.commit;
    setComparableVersions({
      beforeCommitID: newBefore,
      afterCommitID: newAfter,
    });
  }, [afterCommitID, afterIndex, beforeIndex, commit, commits, index, setComparableVersions]);
  const onClickAfter = useCallback(() => {
    // If the currently selected "before" is already older, then keep it.
    // Otherwise, update to the immediate previous version.
    const newBefore = beforeIndex < index ? beforeCommitID : commits[index - 1]?.commit;
    setComparableVersions({
      beforeCommitID: newBefore,
      afterCommitID: commit,
    });
  }, [beforeCommitID, beforeIndex, commit, commits, index, setComparableVersions]);

  return (
    <ActionList.Item sx={{display: 'flex', alignItems: 'center'}}>
      <Box display="flex" alignItems="center" gridGap={2}>
        <Box display="flex" padding={1} gridGap={1}>
          <ToggleButton
            label="Left"
            isSelected={beforeIndex === index}
            onToggle={onClickBefore}
            isDisabled={index === maxIndex}
            width={TOGGLE_BUTTON_WIDTH}
          />
          <ToggleButton
            label="Right"
            isSelected={afterIndex === index}
            onToggle={onClickAfter}
            width={TOGGLE_BUTTON_WIDTH}
          />
        </Box>
        <Box fontSize={0} overflow="hidden">
          <Box lineHeight={1.5}>
            <BulletItems>
              {author && <Text fontWeight="bold">{author}</Text>}
              {formatISODate(committedDate)}
              {commentCount > 0 && <CommentCount count={commentCount} />}
            </BulletItems>
          </Box>
          <Box lineHeight={1.5}>
            <BulletItems>
              <CommitLink org={org} repo={repo} oid={commit} />
              <Box overflow="hidden" sx={{textOverflow: 'ellipsis'}}>
                <Text whiteSpace="nowrap">{message}</Text>
              </Box>
            </BulletItems>
          </Box>
        </Box>
      </Box>
    </ActionList.Item>
  );
});
