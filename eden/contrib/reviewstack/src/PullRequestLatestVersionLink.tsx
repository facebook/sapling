/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {
  gitHubPullRequestComparableVersions,
  gitHubPullRequestIsViewingLatest,
  gitHubPullRequestSelectedVersionIndex,
} from './recoil';
import {ArrowLeftIcon} from '@primer/octicons-react';
import {Link, Text} from '@primer/react';
import {useCallback} from 'react';
import {useRecoilValue, useResetRecoilState} from 'recoil';

export default function PullRequestLatestVersionLink(): React.ReactElement | null {
  const resetSelectedVersionIndex = useResetRecoilState(gitHubPullRequestSelectedVersionIndex);
  const resetComparableVersions = useResetRecoilState(gitHubPullRequestComparableVersions);
  const isViewingLatest = useRecoilValue(gitHubPullRequestIsViewingLatest);

  const onClick = useCallback(() => {
    resetComparableVersions();
    resetSelectedVersionIndex();
  }, [resetComparableVersions, resetSelectedVersionIndex]);

  if (isViewingLatest) {
    return null;
  }

  return (
    <Link as="button" onClick={onClick}>
      <ArrowLeftIcon />
      <Text fontSize={0} fontWeight="bold" marginLeft={1}>
        Back to Latest
      </Text>
    </Link>
  );
}
