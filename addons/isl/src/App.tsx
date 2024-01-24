/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepositoryError} from './types';
import type {ReactNode} from 'react';

import {AccessGlobalRecoil} from './AccessGlobalRecoil';
import {CommandHistoryAndProgress} from './CommandHistoryAndProgress';
import {CommitInfoSidebar} from './CommitInfoView/CommitInfoView';
import {CommitTreeList} from './CommitTreeList';
import {ComparisonViewModal} from './ComparisonView/ComparisonViewModal';
import {CwdSelections} from './CwdSelector';
import {Drawers} from './Drawers';
import {EmptyState} from './EmptyState';
import {ErrorBoundary, ErrorNotice} from './ErrorNotice';
import {ISLCommandContext, useCommand} from './ISLShortcuts';
import {TooltipRootContainer} from './Tooltip';
import {TopBar} from './TopBar';
import {TopLevelErrors} from './TopLevelErrors';
import {TopLevelToast} from './TopLevelToast';
import {tracker} from './analytics';
import {islDrawerState} from './drawerState';
import {GettingStartedModal} from './gettingStarted/GettingStartedModal';
import {I18nSupport, t, T} from './i18n';
import {setJotaiStore} from './jotaiUtils';
import platform from './platform';
import {DEFAULT_RESET_CSS} from './resetStyle';
import {useMainContentWidth, zoomUISettingAtom} from './responsive';
import {applicationinfo, repositoryInfo} from './serverAPIState';
import {themeState} from './theme';
import {ModalContainer} from './useModal';
import {isTest} from './utils';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {Provider, useAtomValue, useSetAtom, useStore} from 'jotai';
import React from 'react';
import {RecoilRoot, useRecoilValue} from 'recoil';
import {ContextMenus} from 'shared/ContextMenu';
import {Icon} from 'shared/Icon';
import {useThrottledEffect} from 'shared/hooks';

import './index.css';

export default function App() {
  return (
    <React.StrictMode>
      <ResetStyle />
      <I18nSupport>
        <RecoilRoot>
          <AccessGlobalRecoil />
          <MaybeWithJotaiRoot>
            <ISLRoot>
              <ISLCommandContext>
                <ErrorBoundary>
                  <ISLDrawers />
                  <TooltipRootContainer />
                  <GettingStartedModal />
                  <ComparisonViewModal />
                  <ModalContainer />
                  <ContextMenus />
                  <TopLevelToast />
                </ErrorBoundary>
              </ISLCommandContext>
            </ISLRoot>
          </MaybeWithJotaiRoot>
        </RecoilRoot>
      </I18nSupport>
    </React.StrictMode>
  );
}

function MaybeWithJotaiRoot({children}: {children: JSX.Element}) {
  if (isTest) {
    // Use a new store when re-mounting so each test (that calls `render(<App />)`)
    // starts with a clean state.
    return (
      <Provider>
        <AccessJotaiRoot />
        {children}
      </Provider>
    );
  } else {
    // Such scoped Provider or store complexity is not needed outside tests.
    return children;
  }
}

function AccessJotaiRoot() {
  const store = useStore();
  setJotaiStore(store);
  return null;
}

function ResetStyle() {
  const resetCSS = platform.theme?.resetCSS ?? DEFAULT_RESET_CSS;
  return resetCSS.length > 0 ? <style>{resetCSS}</style> : null;
}

function ISLRoot({children}: {children: ReactNode}) {
  const theme = useAtomValue(themeState);
  useAtomValue(zoomUISettingAtom);
  return (
    <div
      className={`isl-root ${theme}-theme`}
      onDragEnter={handleDragAndDrop}
      onDragOver={handleDragAndDrop}>
      {children}
    </div>
  );
}

function handleDragAndDrop(e: React.DragEvent<HTMLDivElement>) {
  // VS Code tries to capture drag & drop events to open files. But if you're dragging
  // on ISL, you probably want to do an ImageUpload. Prevent this event from propagating to vscode.
  if (e.dataTransfer?.types?.some(t => t === 'Files')) {
    e.stopPropagation();
    e.preventDefault();
    e.dataTransfer.dropEffect = 'copy';
  }
}

function ISLDrawers() {
  const setDrawerState = useSetAtom(islDrawerState);
  useCommand('ToggleSidebar', () => {
    setDrawerState(state => ({
      ...state,
      right: {...state.right, collapsed: !state.right.collapsed},
    }));
  });

  return (
    <Drawers
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
  useRecoilValue(applicationinfo); // ensure this info is always fetched

  const ref = useMainContentWidth();

  return (
    <div className="main-content-area" ref={ref}>
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
  useThrottledEffect(
    () => {
      if (repoError != null) {
        switch (repoError.type) {
          case 'cwdNotARepository':
            tracker.track('UIEmptyState', {extras: {cwd: repoError.cwd}, errorName: 'InvalidCwd'});
            break;
          case 'invalidCommand':
            tracker.track('UIEmptyState', {
              extras: {command: repoError.command},
              errorName: 'InvalidCommand',
            });
            break;
          case 'unknownError':
            tracker.error('UIEmptyState', 'RepositoryError', repoError.error);
            break;
        }
      }
    },
    1_000,
    [repoError],
  );
  let content;
  if (repoError != null) {
    if (repoError.type === 'cwdNotARepository') {
      content = (
        <>
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
          <CwdSelections dismiss={() => null} />
        </>
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
