/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepoInfo} from './types';
import type {AllDrawersState} from 'shared/Drawers';

import {CommandHistoryAndProgress} from './CommandHistoryAndProgress';
import {CommitInfoSidebar} from './CommitInfo';
import {CommitTreeList} from './CommitTreeList';
import {ComparisonViewModal} from './ComparisonView/ComparisonViewModal';
import {ErrorBoundary} from './ErrorNotice';
import {ISLCommandContext, useCommand} from './ISLShortcuts';
import {Icon} from './Icon';
import {TopBar} from './TopBar';
import {TopLevelErrors} from './TopLevelErrors';
import {repositoryInfo} from './codeReview/CodeReviewInfo';
import {I18nSupport, T} from './i18n';
import {OptionsModal} from './optionsModal';
import {ThemeRoot} from './theme';
import React from 'react';
import {atom, RecoilRoot, useRecoilValue, useSetRecoilState} from 'recoil';
import {Drawers} from 'shared/Drawers';

import './index.css';

export default function App() {
  return (
    <React.StrictMode>
      <I18nSupport>
        <RecoilRoot>
          <ThemeRoot>
            <ISLCommandContext>
              <ErrorBoundary>
                <ISLDrawers />
                <div className="tooltip-root-container" data-testid="tooltip-root-container" />
                <ComparisonViewModal />
                <OptionsModal />
              </ErrorBoundary>
            </ISLCommandContext>
          </ThemeRoot>
        </RecoilRoot>
      </I18nSupport>
    </React.StrictMode>
  );
}

export const islDrawerState = atom<AllDrawersState>({
  key: 'islDrawerState',
  default: {
    right: {size: 500, collapsed: false},
    left: {size: 200, collapsed: true},
    top: {size: 200, collapsed: true},
    bottom: {size: 200, collapsed: true},
  },
});
function ISLDrawers() {
  const setDrawerState = useSetRecoilState(islDrawerState);
  useCommand('ToggleSidebar', () => {
    setDrawerState(state => ({
      ...state,
      right: {...state.right, collapsed: !state.right.collapsed},
    }));
  });

  return (
    <Drawers
      drawerState={islDrawerState}
      rightLabel={
        <>
          <Icon icon="edit" />
          <T>Commit Info</T>
        </>
      }
      right={<CommitInfoSidebar />}
      errorBoundary={ErrorBoundary}>
      <MainContent />
      <CommandHistoryAndProgress />
    </Drawers>
  );
}

function MainContent() {
  const repoInfo = useRecoilValue(repositoryInfo);

  return (
    <div className="main-content-area">
      <TopBar />
      <TopLevelErrors />
      {repoInfo != null && repoInfo.repoRoot == null ? (
        <EmptyState repoInfo={repoInfo} />
      ) : (
        <CommitTreeList />
      )}
    </div>
  );
}

function EmptyState({repoInfo}: {repoInfo: RepoInfo}) {
  return (
    <div className="empty-app-state">
      <h1>
        <T>No repository found</T>
      </h1>
      <p>
        <T replace={{$root: <code>{repoInfo.repoRoot}</code>}}>$root is not a valid repository</T>
      </p>
    </div>
  );
}
