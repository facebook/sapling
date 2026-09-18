/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import './PullRequest.css';

import type {GitHubPullRequestParams} from './jotai';

import CenteredSpinner from './CenteredSpinner';
import DiffView from './DiffView';
import EditablePullRequestDescription from './EditablePullRequestDescription';
import PullRequestChangeCount from './PullRequestChangeCount';
import PullRequestLabels from './PullRequestLabels';
import PullRequestReviewers from './PullRequestReviewers';
import PullRequestSignals from './PullRequestSignals';
import {
  gitHubOrgAndRepoAtom,
  gitHubPullRequestComparableVersionsAtom,
  gitHubPullRequestAtom,
  gitHubPullRequestForParamsAtom,
  gitHubPullRequestIDAtom,
  gitHubPullRequestSelectedVersionIndexAtom,
  gitHubPullRequestVersionDiffAtom,
  gitHubPullRequestComparisonFilesAtom,
  pendingScrollRestoreAtom,
} from './jotai';
import {Box, Flash, Text} from '@primer/react';
import {useAtomValue, useSetAtom} from 'jotai';
import {loadable} from 'jotai/utils';
import {Suspense, useEffect, useMemo, useRef} from 'react';

export default function PullRequest() {
  // Note: comparableVersions sync is handled by JotaiRecoilSync component
  // which properly waits for valid data before syncing

  return (
    <Suspense fallback={<CenteredSpinner />}>
      <div className="PullRequest-container">
        <PullRequestBootstrap />
      </div>
    </Suspense>
  );
}

function PullRequestBootstrap() {
  const number = useAtomValue(gitHubPullRequestIDAtom);
  const orgAndRepo = useAtomValue(gitHubOrgAndRepoAtom);
  if (number != null && orgAndRepo != null) {
    return <PullRequestWithParams params={{orgAndRepo, number}} />;
  } else {
    return <Text>This is not a URL for a pull request.</Text>;
  }
}

function PullRequestWithParams({params}: {params: GitHubPullRequestParams}) {
  // Use loadable to avoid suspending - we want to show the current PR while
  // refreshing in the background
  const loadablePRAtom = useMemo(() => loadable(gitHubPullRequestForParamsAtom(params)), [params]);
  const pullRequestLoadable = useAtomValue(loadablePRAtom);
  const currentPullRequest = useAtomValue(gitHubPullRequestAtom);
  const setPullRequestJotai = useSetAtom(gitHubPullRequestAtom);
  const setComparableVersions = useSetAtom(gitHubPullRequestComparableVersionsAtom);
  const setSelectedVersionIndex = useSetAtom(gitHubPullRequestSelectedVersionIndexAtom);
  const setPendingScrollRestore = useSetAtom(pendingScrollRestoreAtom);
  const paramsKey = `${params.orgAndRepo.org}\0${params.orgAndRepo.repo}\0${params.number}`;
  const loadedParamsKey = useRef<string | null>(null);
  const pullRequest = pullRequestLoadable.state === 'hasData' ? pullRequestLoadable.data : null;
  const isPullRequestNotFound = pullRequestLoadable.state === 'hasData' && pullRequest == null;

  useEffect(() => {
    setComparableVersions(null);
    setSelectedVersionIndex(0);
  }, [paramsKey, setComparableVersions, setSelectedVersionIndex]);

  useEffect(() => {
    if (pullRequest != null) {
      loadedParamsKey.current = paramsKey;
      // Here we should diff the new value with the existing value for the
      // gitHubPullRequestAtom, preserving as many of the original references
      // as possible to limit the number of updates to the dataflow graph,
      // which will short-circuit a bunch off diff'ing React will have to do.
      setPullRequestJotai(pullRequest);
    }
  }, [paramsKey, pullRequest, setPullRequestJotai]);

  // Restore scroll position after pull request data updates.
  // This runs after the effect above updates the atoms, and uses
  // double requestAnimationFrame to wait for React to commit the render
  // and the browser to paint.
  useEffect(() => {
    if (pullRequest != null) {
      // Use double requestAnimationFrame to ensure we restore scroll after
      // React has committed updates AND the browser has finished painting.
      // The first RAF waits for the next frame, the second ensures paint completion.
      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          setPendingScrollRestore(prev => {
            if (prev != null) {
              window.scrollTo(prev.scrollX, prev.scrollY);
            }
            return null;
          });
        });
      });
    }
  }, [pullRequest, setPendingScrollRestore]);

  if (pullRequestLoadable.state === 'hasError') {
    const error = pullRequestLoadable.error;
    return (
      <Flash variant="danger" role="alert">
        <Text as="p" fontWeight="bold">
          Could not load this pull request.
        </Text>
        <Box as="pre" sx={{whiteSpace: 'pre-wrap', overflowWrap: 'anywhere'}}>
          {error instanceof Error ? error.message : String(error)}
        </Box>
        <Text as="p">
          Check that your GitHub token covers this repository and can read its contents and pull
          requests. After changing permissions, reload this page.
        </Text>
      </Flash>
    );
  } else if (
    pullRequestLoadable.state === 'loading' &&
    (currentPullRequest == null || loadedParamsKey.current !== paramsKey)
  ) {
    return <CenteredSpinner />;
  } else if (isPullRequestNotFound) {
    return <PullRequestNotFound />;
  } else {
    return <PullRequestDetails />;
  }
}

function PullRequestNotFound() {
  return <Text>The specified pull request could not be found.</Text>;
}

function PullRequestDetails() {
  const pullRequest = useAtomValue(gitHubPullRequestAtom);
  if (pullRequest == null) {
    return null;
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
        <EditablePullRequestDescription />
      </Box>
      <PullRequestSignals />
      <Suspense fallback={<CenteredSpinner />}>
        <div>
          <div
            style={{
              display: 'flex',
              flexDirection: 'row',
              gap: '.5rem',
              paddingBottom: '.5rem',
            }}>
            <PullRequestChangeCount />
          </div>
          <PullRequestVersionDiff />
        </div>
      </Suspense>
    </Box>
  );
}

function PullRequestVersionDiff() {
  const diff = useAtomValue(gitHubPullRequestVersionDiffAtom);
  const comparisonFiles = useAtomValue(gitHubPullRequestComparisonFilesAtom);

  if (diff != null) {
    return (
      <Suspense
        fallback={<CenteredSpinner message={'Loading ' + diff.diff.length + ' changes...'} />}>
        <DiffView diff={diff.diff} isPullRequest={true} comparisonFiles={comparisonFiles} />
      </Suspense>
    );
  } else {
    return null;
  }
}
