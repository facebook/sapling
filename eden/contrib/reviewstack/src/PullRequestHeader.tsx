/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import PullRequestDraftStateMenu from './PullRequestDraftStateMenu';
import PullRequestStack from './PullRequestStack';
import PullRequestVersions from './PullRequestVersions';
import TrustedRenderedMarkdown from './TrustedRenderedMarkdown';
import {gitHubPullRequestAtom} from './jotai';
import {Box, Link, Text} from '@primer/react';
import {useAtomValue} from 'jotai';
import {Suspense} from 'react';

type Props = {
  height: number;
};

export default function PullRequestHeader({height}: Props): React.ReactElement | null {
  const pullRequest = useAtomValue(gitHubPullRequestAtom);

  if (pullRequest == null) {
    return null;
  }

  const {id, isDraft, number, reviewDecision, state, titleHTML, url, viewerCanUpdate} = pullRequest;

  return (
    <Box
      height={height}
      borderBottomWidth={1}
      borderBottomStyle="solid"
      borderBottomColor="border.default"
      display="flex"
      flexDirection="column"
      gridGap={2}
      padding={3}
      position="relative"
      zIndex={100}>
      <Box fontWeight="bold">
        #{number} <TrustedRenderedMarkdown trustedHTML={titleHTML} inline={true} />{' '}
        <Link href={url} target="_blank">
          <Text fontWeight="normal">(view on GitHub)</Text>
        </Link>
      </Box>
      <Box display="flex" gridGap={2}>
        <PullRequestDraftStateMenu
          id={id}
          isDraft={isDraft}
          reviewDecision={reviewDecision ?? null}
          state={state}
          viewerCanUpdate={viewerCanUpdate}
        />
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
