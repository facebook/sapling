/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {GitObjectID, ID} from './github/types';

import {
  gitHubPullRequestComparableVersions,
  gitHubPullRequestJumpToCommentID,
  gitHubPullRequestSelectedVersionIndex,
  gitHubPullRequestVersionIndexForCommit,
} from './recoil';
import {Box, Link, Text} from '@primer/react';
import React, {useCallback} from 'react';
import {useRecoilValue, useSetRecoilState} from 'recoil';

type Props = {
  commentID: ID;
  commit: GitObjectID;
  lineNumber: number;
};

// eslint-disable-next-line prefer-arrow-callback
export default React.memo(function PullRequestReviewCommentLineNumber({
  commentID,
  commit,
  lineNumber,
}: Props): React.ReactElement {
  const versionIndex = useRecoilValue(gitHubPullRequestVersionIndexForCommit(commit));
  const setJumpToCommentID = useSetRecoilState(gitHubPullRequestJumpToCommentID(commentID));
  const setSelectedVersionIndex = useSetRecoilState(gitHubPullRequestSelectedVersionIndex);
  const setComparableVersions = useSetRecoilState(gitHubPullRequestComparableVersions);

  const onClick = useCallback(() => {
    if (versionIndex != null) {
      setJumpToCommentID(true);
      setSelectedVersionIndex(versionIndex);
      setComparableVersions({
        beforeCommitID: null,
        afterCommitID: commit,
      });
    }
  }, [commit, setComparableVersions, setJumpToCommentID, setSelectedVersionIndex, versionIndex]);

  return (
    <Box lineHeight={1}>
      <Link as="button" onClick={onClick}>
        <Text fontSize={1}>{lineNumber}</Text>
      </Link>
    </Box>
  );
});
