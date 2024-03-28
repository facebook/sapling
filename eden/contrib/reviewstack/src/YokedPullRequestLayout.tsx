/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {AllDrawersState} from 'shared/Drawers';

import CenteredSpinner from './CenteredSpinner';
import {useCommand} from './KeyboardShortcuts';
import YokedPullRequest from './YokedPullRequest';
import YokedPullRequestHeader from './YokedPullRequestHeader';
import YokedPullRequestTimeline from './YokedPullRequestTimeline';
import YokedPullRequestTimelineCommentInput from './YokedPullRequestTimelineCommentInput';
import {APP_HEADER_HEIGHT} from './constants';
import {gitHubOrgAndRepo, gitHubPullRequestID} from './recoil';
import {Box, Text} from '@primer/react';
import React, {Component, Suspense, useEffect, useRef, RefObject} from 'react';
import {atom, useSetRecoilState} from 'recoil';
import {Drawers} from 'shared/Drawers';

import './PullRequestLayout.css';

import YokedPullRequestStack from './YokedPullRequestStack';
import {HomeIcon, CommentDiscussionIcon} from '@primer/octicons-react';
import {StyledOcticon} from '@primer/react';

const HEADER_HEIGHT = 121;
const TOTAL_HEADER_HEIGHT = HEADER_HEIGHT + APP_HEADER_HEIGHT;
const COMMENT_INPUT_HEIGHT = 148;

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

  const onToggleDrawer = () => {
    console.log('test');
    setDrawerState(state => ({
      ...state,
      right: {...state.right, collapsed: !state.right.collapsed},
    }));
  };

  const prHeaderElement: RefObject<HTMLDivElement> = useRef(null);
  const prHeaderHeight = prHeaderElement?.current
    ? prHeaderElement.current.getBoundingClientRect().height
    : 0;
  const totalHeaderHeight = APP_HEADER_HEIGHT + prHeaderHeight;

  // console.log('prHeaderHeight', prHeaderHeight);
  // console.log('HEADER_HEIGHT', HEADER_HEIGHT);

  return (
    <div className="yoke">
      <div className="yoke-aside">
        <YokedPullRequestStack />
        {/* <div className="stack-items">
          <div className="stack-item">
            <div className="stack-item-order">01</div>
            <div className="stack-item-info">
              <div className="stack-item-title">
                <span>"StandardCharsets" constants should be preferred.</span>
              </div>
              <div className="stack-item-id">#123</div>
            </div>
            <button className="stack-item-handle" />
          </div>
          <div className="stack-item active">
            <div className="stack-item-order">02</div>
            <div className="stack-item-info">
              <div className="stack-item-title">
                <span className="stack-item-name">
                  Assertion arguments should be passed in the correct order.
                </span>
                <span className="stack-item-id">#124</span>
              </div>
              <div className="stack-item-description">
                (r4e-address/S3415) https://rules.sonarsource.com/java/RSPEC-3415/
              </div>
            </div>
          </div>
          <div className="stack-item">
            <div className="stack-item-order">03</div>
            <div className="stack-item-info">
              <div className="stack-item-title">
                <span>{`The diamond operator ("<>") should be used.`}</span>
              </div>
              <div className="stack-item-id">#125</div>
            </div>
          </div>
        </div> */}
      </div>
      <div className="yoke-main">
        <div className="yoke-main-inner">
          <YokedPullRequestHeader prHeaderElement={prHeaderElement} />
          <div className="yoke-body">
            <Suspense fallback={<CenteredSpinner message="Loading pull request..." />}>
              <Drawers
                drawerState={drawerState}
                errorBoundary={ErrorBoundary}
                rightLabel={<StyledOcticon icon={CommentDiscussionIcon} />}
                right={<TimelineDrawer />}>
                <Box display="flex" flexDirection="row">
                  <Box height={`calc(100vh - ${TOTAL_HEADER_HEIGHT}px)`}>
                    <YokedPullRequest />
                  </Box>
                </Box>
              </Drawers>
            </Suspense>
            <button className="yoke-drawer-toggle" onClick={onToggleDrawer}>
              <StyledOcticon icon={CommentDiscussionIcon} />
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

function TimelineDrawer() {
  return (
    <Box display="flex" flexDirection="column" height={`calc(100vh - ${TOTAL_HEADER_HEIGHT}px)`}>
      <Box overflow="auto" flex={1}>
        <YokedPullRequestTimeline />
      </Box>
      <Box display="flex" height={COMMENT_INPUT_HEIGHT}>
        <YokedPullRequestTimelineCommentInput />
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
