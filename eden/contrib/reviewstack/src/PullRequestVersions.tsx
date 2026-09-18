/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import PullRequestLatestVersionLink from './PullRequestLatestVersionLink';
import PullRequestVersionCommitSelector from './PullRequestVersionCommitSelector';
import PullRequestVersionSelector from './PullRequestVersionSelector';
import {
  gitHubOrgAndRepoAtom,
  gitHubPullRequestComparableVersionsAtom,
  gitHubPullRequestSelectedVersionIndexAtom,
  gitHubPullRequestVersionsAtom,
} from './jotai';
import {Box} from '@primer/react';
import {useAtomValue, useSetAtom} from 'jotai';
import {useEffect} from 'react';

export default function PullRequestVersions(): React.ReactElement | null {
  const {org, repo} = useAtomValue(gitHubOrgAndRepoAtom) ?? {};
  const versions = useAtomValue(gitHubPullRequestVersionsAtom);
  const setComparableVersions = useSetAtom(gitHubPullRequestComparableVersionsAtom);
  const setSelectedVersionIndex = useSetAtom(gitHubPullRequestSelectedVersionIndexAtom);
  const latestVersion = versions[versions.length - 1];
  const latestHeadCommit = latestVersion?.headCommit;

  useEffect(() => {
    if (latestHeadCommit == null) {
      return;
    }
    setSelectedVersionIndex(versions.length - 1);
    setComparableVersions({beforeCommitID: null, afterCommitID: latestHeadCommit});
  }, [latestHeadCommit, setComparableVersions, setSelectedVersionIndex, versions.length]);

  if (org == null || repo == null) {
    return null;
  }

  return (
    <Box display="flex" alignItems="center" gridGap={2}>
      <PullRequestVersionSelector org={org} repo={repo} />
      <PullRequestVersionCommitSelector org={org} repo={repo} />
      <PullRequestLatestVersionLink />
    </Box>
  );
}
