/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import './PullRequest.css';

import type {GitHubPullRequestParams} from './recoil';

import CenteredSpinner from './CenteredSpinner';
import DiffView from './DiffView';
import PullRequestLabels from './PullRequestLabels';
import PullRequestReviewers from './PullRequestReviewers';
import PullRequestSignals from './PullRequestSignals';
import TrustedRenderedMarkdown from './TrustedRenderedMarkdown';
import {stripStackInfoFromBodyHTML} from './ghstackUtils';
import {
  gitHubPullRequest,
  gitHubOrgAndRepo,
  gitHubPullRequestForParams,
  gitHubPullRequestID,
  gitHubPullRequestComparableVersions,
  gitHubPullRequestVersionDiff,
} from './recoil';
import {stripStackInfoFromSaplingBodyHTML} from './saplingStack';
import {stackedPullRequest} from './stackState';
import {Box, Text} from '@primer/react';
import {Suspense, useEffect} from 'react';
import {
  useRecoilValue,
  useRecoilValueLoadable,
  useResetRecoilState,
  useSetRecoilState,
} from 'recoil';

export default function PullRequest() {
  const resetComparableVersions = useResetRecoilState(gitHubPullRequestComparableVersions);
  // Reset the radio buttons as part of the initial page load.
  useEffect(() => {
    resetComparableVersions();
  }, [resetComparableVersions]);

  return (
    <Suspense fallback={<CenteredSpinner />}>
      <div className="PullRequest-container">
        <PullRequestBootstrap />
      </div>
    </Suspense>
  );
}

function PullRequestBootstrap() {
  const number = useRecoilValue(gitHubPullRequestID);
  const orgAndRepo = useRecoilValue(gitHubOrgAndRepo);
  if (number != null && orgAndRepo != null) {
    return <PullRequestWithParams params={{orgAndRepo, number}} />;
  } else {
    return <Text>This is not a URL for a pull request.</Text>;
  }
}

function PullRequestWithParams({params}: {params: GitHubPullRequestParams}) {
  // When useRefreshPullRequest() is used to update gitHubPullRequestForParams,
  // we expect *most* of the data that comes back to be the same as before.
  // As such, we would prefer to avoid triggering <Suspense>, as the user would
  // briefly see a loading indicator followed by a massive redraw to restore
  // what they were just looking at. To avoid this, we leverage
  // useRecoilValueLoadable() to probe for updates to gitHubPullRequestForParams
  // while using the gitHubPullRequest for the purposes of rendering, as it is
  // updated synchronously and therefore will not trigger <Suspense>.
  const pullRequestLoadable = useRecoilValueLoadable(gitHubPullRequestForParams(params));
  const setPullRequest = useSetRecoilState(gitHubPullRequest);
  const pullRequest =
    pullRequestLoadable.state === 'hasValue' ? pullRequestLoadable.contents : null;
  const isPullRequestNotFound = pullRequestLoadable.state === 'hasValue' && pullRequest == null;

  useEffect(() => {
    if (pullRequest != null) {
      // Here we should diff the new value with the existing value for the
      // gitHubPullRequest atom, preserving as many of the original references
      // as possible to limit the number of updates to the dataflow graph,
      // which will short-circuit a bunch off diff'ing React will have to do.
      setPullRequest(pullRequest);
    }
  }, [pullRequest, setPullRequest]);
  if (isPullRequestNotFound) {
    return <PullRequestNotFound />;
  } else {
    return <PullRequestDetails />;
  }
}

function PullRequestNotFound() {
  return <Text>The specified pull request could not be found.</Text>;
}

function PullRequestDetails() {
  const pullRequest = useRecoilValue(gitHubPullRequest);
  const pullRequestStack = useRecoilValueLoadable(stackedPullRequest);
  if (pullRequest == null || pullRequestStack.state !== 'hasValue') {
    return null;
  }

  const stack = pullRequestStack.contents;
  const {bodyHTML} = pullRequest;
  let pullRequestBodyHTML;
  switch (stack.type) {
    case 'no-stack':
      pullRequestBodyHTML = bodyHTML;
      break;
    case 'sapling':
      pullRequestBodyHTML = stripStackInfoFromSaplingBodyHTML(bodyHTML);
      break;
    case 'ghstack':
      pullRequestBodyHTML = stripStackInfoFromBodyHTML(bodyHTML);
      break;
  }

  return (
    <Box display="flex" flexDirection="column" paddingTop={3} gridGap={3}>
      <PullRequestReviewers />
      <PullRequestLabels />
      <Box
        borderWidth={1}
        borderStyle="solid"
        borderColor="accent.muted"
        borderRadius={4}
        fontSize={14}
        padding={3}>
        <TrustedRenderedMarkdown trustedHTML={pullRequestBodyHTML} />
      </Box>
      <PullRequestSignals />
      <Suspense fallback={<CenteredSpinner />}>
        <PullRequestVersionDiff />
      </Suspense>
    </Box>
  );
}

function PullRequestVersionDiff() {
  const loadable = useRecoilValueLoadable(gitHubPullRequestVersionDiff);
  const diff = loadable.valueMaybe();

  if (diff != null) {
    return <DiffView diff={diff.diff} isPullRequest={true} />;
  } else {
    return null;
  }
}
