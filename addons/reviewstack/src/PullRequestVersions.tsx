/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import PullRequestLatestVersionLink from './PullRequestLatestVersionLink';
import PullRequestVersionCommitSelector from './PullRequestVersionCommitSelector';
import PullRequestVersionSelector from './PullRequestVersionSelector';
import {gitHubOrgAndRepo} from './recoil';
import {Box} from '@primer/react';
import {useRecoilValue} from 'recoil';

export default function PullRequestVersions(): React.ReactElement | null {
  const {org, repo} = useRecoilValue(gitHubOrgAndRepo) ?? {};

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
