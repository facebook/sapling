/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import PullRequestStack from './PullRequestStack';
import PullRequestStateLabel from './PullRequestStateLabel';
import PullRequestVersions from './PullRequestVersions';
import TrustedRenderedMarkdown from './TrustedRenderedMarkdown';
import {gitHubPullRequest} from './recoil';
import {Box, Link, Text} from '@primer/react';
import {Suspense} from 'react';
import {useRecoilValue} from 'recoil';

type Props = {
  height: number;
};

export default function PullRequestHeader({height}: Props): React.ReactElement | null {
  const pullRequest = useRecoilValue(gitHubPullRequest);

  if (pullRequest == null) {
    return null;
  }

  const {number, reviewDecision, state, titleHTML, url} = pullRequest;

  return (
    <Box
      height={height}
      borderBottomWidth={1}
      borderBottomStyle="solid"
      borderBottomColor="border.default"
      display="flex"
      flexDirection="column"
      gridGap={2}
      padding={3}>
      <Box fontWeight="bold">
        #{number} <TrustedRenderedMarkdown trustedHTML={titleHTML} inline={true} />{' '}
        <Link href={url} target="_blank">
          <Text fontWeight="normal">(view on GitHub)</Text>
        </Link>
      </Box>
      <Box display="flex" gridGap={2}>
        <PullRequestStateLabel reviewDecision={reviewDecision ?? null} state={state} />
        <PullRequestStack />
        {/*
          Our goal here is to minimize re-rendering when the user selects a
          different value from <PullRequestStack>, so we apply <Suspense> in a
          very narrow context.

          Ideally, we would update <PullRequestVersions> so it never needs a
          <Suspend>, leveraging useRecoilValueLoadable() as we did in
          <PullRequestStack> because Recoil wakes all suspended components
          whenever any async selector is resolved, so every use of <Suspense>
          runs the risk of a hard-to-debug performance issue.
          */}
        <Suspense fallback={null}>
          <PullRequestVersions />
        </Suspense>
      </Box>
    </Box>
  );
}
