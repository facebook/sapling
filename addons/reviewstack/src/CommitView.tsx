/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Commit} from './github/types';

import CenteredSpinner from './CenteredSpinner';
import CommitHeader from './CommitHeader';
import CommitLink from './CommitLink';
import DiffView from './DiffView';
import TrustedRenderedMarkdown from './TrustedRenderedMarkdown';
import {
  gitHubCommitID,
  gitHubCurrentCommit,
  gitHubDiffForCurrentCommit,
  gitHubOrgAndRepo,
} from './recoil';
import {Box, Text} from '@primer/react';
import {Suspense, useEffect} from 'react';
import {useRecoilValue, useSetRecoilState} from 'recoil';

export default function CommitView({org, repo, oid}: {org: string; repo: string; oid: string}) {
  const setOrgAndRepo = useSetRecoilState(gitHubOrgAndRepo);
  const setCommitID = useSetRecoilState(gitHubCommitID);

  useEffect(() => {
    setOrgAndRepo({org, repo});
  }, [org, repo, setOrgAndRepo]);

  useEffect(() => {
    setCommitID(oid);
  }, [oid, setCommitID]);

  return (
    <Box>
      <Suspense fallback={<CenteredSpinner message="Loading commit..." />}>
        <CommitHeader />
        <CommitDisplay />
      </Suspense>
    </Box>
  );
}

function CommitDisplay() {
  const diff = useRecoilValue(gitHubDiffForCurrentCommit);
  const commit = useRecoilValue(gitHubCurrentCommit);

  if (diff != null) {
    return (
      <Box>
        <Box marginX="6px" overflowX="auto">
          {commit && <CommitMessage commit={commit} />}
          <DiffView diff={diff.diff} isPullRequest={false} />
        </Box>
      </Box>
    );
  } else {
    return (
      <Box>
        <Text>commit not found or fetched from GitHub URL above</Text>
      </Box>
    );
  }
}

function CommitMessage({commit}: {commit: Commit}) {
  return (
    <Box mb={1}>
      <TrustedRenderedMarkdown trustedHTML={commit.messageBodyHTML} />
      <CommitParents commit={commit} />
    </Box>
  );
}

function CommitParents({commit}: {commit: Commit}) {
  const {org, repo} = useRecoilValue(gitHubOrgAndRepo) ?? {};
  const {parents} = commit;
  if (parents.length === 0 || org == null || repo == null) {
    return null;
  }

  const children = parents.map(parent => (
    <Box key={parent} fontSize={12}>
      Parent: <CommitLink org={org} repo={repo} oid={parent} />
    </Box>
  ));
  return <Box>{children}</Box>;
}
