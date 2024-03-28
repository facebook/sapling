/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import PullRequestStack from './PullRequestStack';
import YokedPullRequestStateLabel from './YokedPullRequestStateLabel';
import YokedPullRequestLabels from './YokedPullRequestLabels';
import YokedPullRequestReviewers from './YokedPullRequestReviewers';
import YokedPullRequestSignals from './YokedPullRequestSignals';
import PullRequestVersions from './PullRequestVersions';
import TrustedRenderedMarkdown from './TrustedRenderedMarkdown';
import {gitHubPullRequest} from './recoil';
import {Box, Label, Link, Text} from '@primer/react';
import {Suspense, useRef, RefObject} from 'react';
import {useRecoilValue} from 'recoil';

type Props = {
  prHeaderElement: RefObject<HTMLDivElement>;
};

export default function PullRequestHeader({prHeaderElement}: Props): React.ReactElement | null {
  const pullRequest = useRecoilValue(gitHubPullRequest);

  if (pullRequest == null) {
    return null;
  }

  const {number, reviewDecision, state, titleHTML, url} = pullRequest;

  return (
    <Box
      className="pr-header"
      borderBottomWidth={1}
      borderBottomStyle="solid"
      borderBottomColor="border.default"
      display="flex"
      flexDirection="column"
      gridGap={2}
      ref={prHeaderElement}
      // ref={prHeaderRef}
    >
      <Box>
        <TrustedRenderedMarkdown trustedHTML={titleHTML} inline={true} className="pr-title" />{' '}
        <Link className="pr-number" href={url} target="_blank">
          <Text fontWeight="normal">{`#${number}`}</Text>
        </Link>
      </Box>
      <Box display="flex" flexWrap={'wrap'} gridGap={2}>
        <YokedPullRequestStateLabel reviewDecision={reviewDecision ?? null} state={state} />
        <YokedPullRequestSignals />
        <YokedPullRequestReviewers />
        <YokedPullRequestLabels />

        {/* NOTE(dk): No longer needed with Yoke sidebar */}
        {/* <PullRequestStack /> */}

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
        {/* NOTE(dk) : Hide versioning and left/right view options for now */}
        {/* <Suspense fallback={null}>
          <PullRequestVersions />
        </Suspense> */}
      </Box>
    </Box>
  );
}
