/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepositoryError} from './types';
import type {AllDrawersState} from 'shared/Drawers';

import {CommandHistoryAndProgress} from './CommandHistoryAndProgress';
import {CommitInfoSidebar} from './CommitInfo';
import {CommitTreeList} from './CommitTreeList';
import {ComparisonViewModal} from './ComparisonView/ComparisonViewModal';
import {EmptyState} from './EmptyState';
import {ErrorBoundary, ErrorNotice} from './ErrorNotice';
import {ISLCommandContext, useCommand} from './ISLShortcuts';
import {Icon} from './Icon';
import {TopBar} from './TopBar';
import {TopLevelErrors} from './TopLevelErrors';
import {I18nSupport, t, T} from './i18n';
import {OptionsModal} from './optionsModal';
import platform from './platform';
import {repositoryInfo} from './serverAPIState';
import {ThemeRoot} from './theme';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
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
      {repoInfo != null && repoInfo.type !== 'success' ? (
        <ISLNullState repoError={repoInfo} />
      ) : (
        <CommitTreeList />
      )}
    </div>
  );
}

function ISLNullState({repoError}: {repoError: RepositoryError}) {
  let content;
  if (repoError != null) {
    if (repoError.type === 'cwdNotARepository') {
      content = (
        <EmptyState>
          <div>
            <T>Not a valid repository</T>
          </div>
          <p>
            <T replace={{$cwd: <code>{repoError.cwd}</code>}}>
              $cwd is not a valid Sapling repository. Clone or init a repository to use ISL.
            </T>
          </p>
        </EmptyState>
      );
    } else if (repoError.type === 'invalidCommand') {
      content = (
        <ErrorNotice
          title={<T>Invalid Sapling command. Is Sapling installed correctly?</T>}
          error={
            new Error(t('Command "$cmd" was not found.', {replace: {$cmd: repoError.command}}))
          }
          buttons={[
            <VSCodeButton
              key="help-button"
              appearance="secondary"
              onClick={e => {
                platform.openExternalLink('https://sapling-scm.com/docs/introduction/installation');
                e.preventDefault();
                e.stopPropagation();
              }}>
              <T>See installation docs</T>
            </VSCodeButton>,
          ]}
        />
      );
    } else {
      content = <ErrorNotice title={<T>Something went wrong</T>} error={repoError.error} />;
    }
  }

  return <div className="empty-app-state">{content}</div>;
}
