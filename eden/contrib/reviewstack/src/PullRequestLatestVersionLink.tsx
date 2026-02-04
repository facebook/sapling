/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {
  gitHubPullRequestComparableVersionsAtom,
  gitHubPullRequestSelectedVersionIndexAtom,
  gitHubPullRequestVersionsAtom,
} from './jotai';
import {gitHubPullRequestIsViewingLatest} from './recoil';
import {ArrowLeftIcon} from '@primer/octicons-react';
import {Link, Text} from '@primer/react';
import {useAtomValue, useSetAtom} from 'jotai';
import {useCallback} from 'react';
import {useRecoilValue} from 'recoil';

export default function PullRequestLatestVersionLink(): React.ReactElement | null {
  const versions = useAtomValue(gitHubPullRequestVersionsAtom);
  const setSelectedVersionIndex = useSetAtom(gitHubPullRequestSelectedVersionIndexAtom);
  const setComparableVersions = useSetAtom(gitHubPullRequestComparableVersionsAtom);
  const isViewingLatest = useRecoilValue(gitHubPullRequestIsViewingLatest);

  const onClick = useCallback(() => {
    // Reset to latest version
    const latestVersionIndex = Math.max(0, versions.length - 1);
    const latestVersion = versions[latestVersionIndex];
    setSelectedVersionIndex(latestVersionIndex);
    if (latestVersion != null) {
      setComparableVersions({
        beforeCommitID: latestVersion.baseParent,
        afterCommitID: latestVersion.headCommit,
      });
    }
  }, [versions, setSelectedVersionIndex, setComparableVersions]);

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
