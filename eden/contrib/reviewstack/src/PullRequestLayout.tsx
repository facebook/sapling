/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {AllDrawersState} from 'shared/Drawers';

import CenteredSpinner from './CenteredSpinner';
import {useCommand} from './KeyboardShortcuts';
import PullRequest from './PullRequest';
import PullRequestHeader from './PullRequestHeader';
import PullRequestTimeline from './PullRequestTimeline';
import PullRequestTimelineCommentInput from './PullRequestTimelineCommentInput';
import {APP_HEADER_HEIGHT} from './constants';
import {gitHubOrgAndRepo, gitHubPullRequestID} from './recoil';
import {Box, Text} from '@primer/react';
import React, {Component, Suspense, useEffect} from 'react';
import {atom, useSetRecoilState} from 'recoil';
import {Drawers} from 'shared/Drawers';

import './PullRequestLayout.css';

const HEADER_HEIGHT = 100;
const TOTAL_HEADER_HEIGHT = HEADER_HEIGHT + APP_HEADER_HEIGHT;
const COMMENT_INPUT_HEIGHT = 125;

const drawerState = atom<AllDrawersState>({
  key: 'drawerState',
  default: {
    right: {size: 500, collapsed: false},
    left: {size: 200, collapsed: true},
    top: {size: 200, collapsed: true},
    bottom: {size: 200, collapsed: true},
  },
});

export default function PullRequestLayout({
  org,
  repo,
  number,
}: {
  org: string;
  repo: string;
  number: number;
}): React.ReactElement {
  const setOrgAndRepo = useSetRecoilState(gitHubOrgAndRepo);
  const setPullRequestID = useSetRecoilState(gitHubPullRequestID);

  useEffect(() => {
    setOrgAndRepo({org, repo});
  }, [org, repo, setOrgAndRepo]);

  useEffect(() => {
    setPullRequestID(number);
  }, [number, setPullRequestID]);

  const setDrawerState = useSetRecoilState(drawerState);
  useCommand('ToggleSidebar', () => {
    setDrawerState(state => ({
      ...state,
      right: {...state.right, collapsed: !state.right.collapsed},
    }));
  });

  return (
    <Box>
      <PullRequestHeader height={HEADER_HEIGHT} />
      <Suspense fallback={<CenteredSpinner message="Loading pull request..." />}>
        <Drawers
          drawerState={drawerState}
          errorBoundary={ErrorBoundary}
          rightLabel={<Text className="drawer-label-text">...</Text>}
          right={<TimelineDrawer />}>
          <Box display="flex" flexDirection="row">
            <Box height={`calc(100vh - ${TOTAL_HEADER_HEIGHT}px)`} overflow="auto">
              <PullRequest />
            </Box>
          </Box>
        </Drawers>
      </Suspense>
    </Box>
  );
}

function TimelineDrawer() {
  return (
    <Box display="flex" flexDirection="column" height={`calc(100vh - ${TOTAL_HEADER_HEIGHT}px)`}>
      <Box height={`calc(100% - ${COMMENT_INPUT_HEIGHT}px)`} overflow="auto">
        <PullRequestTimeline />
      </Box>
      <Box display="flex" height={COMMENT_INPUT_HEIGHT}>
        <PullRequestTimelineCommentInput />
      </Box>
    </Box>
  );
}

type Props = {
  children: React.ReactNode;
};

type State = {error: Error | null};

class ErrorBoundary extends Component<Props, State> {
  static getDerivedStateFromError(error: Error) {
    return {error};
  }

  render() {
    return this.props.children;
  }
}
