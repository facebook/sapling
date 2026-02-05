/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {GitObjectID, ID} from './github/types';

import {
  gitHubPullRequestComparableVersionsAtom,
  gitHubPullRequestJumpToCommentIDAtom,
  gitHubPullRequestSelectedVersionIndexAtom,
  gitHubPullRequestVersionIndexForCommitAtom,
} from './jotai';
import {Box, Link, Text} from '@primer/react';
import {useAtomValue, useSetAtom} from 'jotai';
import React, {useCallback} from 'react';

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
  const versionIndex = useAtomValue(gitHubPullRequestVersionIndexForCommitAtom(commit));
  const setJumpToCommentID = useSetAtom(gitHubPullRequestJumpToCommentIDAtom(commentID));
  const setSelectedVersionIndex = useSetAtom(gitHubPullRequestSelectedVersionIndexAtom);
  const setComparableVersions = useSetAtom(gitHubPullRequestComparableVersionsAtom);

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
