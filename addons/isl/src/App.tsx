/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepositoryError} from './types';
import type {ReactNode} from 'react';
import type {Writable} from 'shared/typeUtils';

import {CommandHistoryAndProgress} from './CommandHistoryAndProgress';
import {CommitInfoSidebar} from './CommitInfoView/CommitInfoView';
import {CommitTreeList} from './CommitTreeList';
import {ComparisonViewModal} from './ComparisonView/ComparisonViewModal';
import {CwdSelections} from './CwdSelector';
import {Drawers} from './Drawers';
import {EmptyState} from './EmptyState';
import {ErrorBoundary, ErrorNotice} from './ErrorNotice';
import {ISLCommandContext, useCommand} from './ISLShortcuts';
import {Internal} from './Internal';
import {TooltipRootContainer} from './Tooltip';
import {TopBar} from './TopBar';
import {TopLevelAlerts} from './TopLevelAlert';
import {TopLevelErrors} from './TopLevelErrors';
import {TopLevelToast} from './TopLevelToast';
import {tracker} from './analytics';
import {enableReduxTools} from './debug/DebugToolsMenu';
import {islDrawerState} from './drawerState';
import {GettingStartedModal} from './gettingStarted/GettingStartedModal';
import {I18nSupport, t, T} from './i18n';
import {setJotaiStore} from './jotaiUtils';
import platform from './platform';
import {DEFAULT_RESET_CSS} from './resetStyle';
import {useMainContentWidth, zoomUISettingAtom} from './responsive';
import {applicationinfo, repositoryInfo} from './serverAPIState';
import {themeState} from './theme';
import {useAtomsDevtools} from './third-party/jotai-devtools/utils';
import {light} from './tokens.stylex';
import {ModalContainer} from './useModal';
import {isDev, isTest} from './utils';
import * as stylex from '@stylexjs/stylex';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {Provider, useAtom, useAtomValue, useSetAtom, useStore} from 'jotai';
import React from 'react';
import {ContextMenus} from 'shared/ContextMenu';
import {Icon} from 'shared/Icon';
import {useThrottledEffect} from 'shared/hooks';

import './index.css';

export default function App() {
  return (
    <React.StrictMode>
      <ResetStyle />
      <I18nSupport>
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
  } else if (isDev) {
    return <MaybeAtomsDevtools>{children}</MaybeAtomsDevtools>;
  } else {
    // Such scoped Provider or store complexity is not needed outside tests or dev.
    return children;
  }
}

function MaybeAtomsDevtools({children}: {children: JSX.Element}) {
  const enabled = useAtomValue(enableReduxTools);
  return enabled ? <AtomsDevtools>{children}</AtomsDevtools> : children;
}

function AtomsDevtools({children}: {children: JSX.Element}) {
  useAtomsDevtools('jotai');
  return children;
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
  const props = stylex.props(theme === 'light' && light);
  // stylex would overwrite className
  (props as Writable<typeof props>).className += ` isl-root ${theme}-theme`;
  return (
    <div onDragEnter={handleDragAndDrop} onDragOver={handleDragAndDrop}>
      <div {...props}>{children}</div>
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
  const repoInfo = useAtomValue(repositoryInfo);
  useAtom(applicationinfo); // ensure this info is always fetched

  const ref = useMainContentWidth();

  return (
    <div className="main-content-area" ref={ref}>
      <TopBar />
      <TopLevelErrors />
      <TopLevelAlerts />
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
    } else if (repoError.type === 'cwdDoesNotExist') {
      content = (
        <ErrorNotice
          title={
            <T replace={{$cwd: repoError.cwd}}>
              cwd $cwd does not exist. Make sure the folder exists.
            </T>
          }
          error={
            new Error(
              t('$cwd not found', {
                replace: {$cwd: repoError.cwd},
              }),
            )
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
    } else if (repoError.type === 'invalidCommand') {
      if (Internal.InvalidSlCommand) {
        content = <Internal.InvalidSlCommand repoError={repoError} />;
      } else {
        content = (
          <ErrorNotice
            startExpanded
            title={<T>Invalid Sapling command. Is Sapling installed correctly?</T>}
            description={
              <T replace={{$cmd: repoError.command}}>Command "$cmd" was not found in PATH</T>
            }
            details={<T replace={{$path: repoError.path ?? '(no path found)'}}>PATH: $path'</T>}
            buttons={[
              <VSCodeButton
                key="help-button"
                appearance="secondary"
                onClick={e => {
                  platform.openExternalLink(
                    'https://sapling-scm.com/docs/introduction/installation',
                  );
                  e.preventDefault();
                  e.stopPropagation();
                }}>
                <T>See installation docs</T>
              </VSCodeButton>,
            ]}
          />
        );
      }
    } else {
      content = <ErrorNotice title={<T>Something went wrong</T>} error={repoError.error} />;
    }
  }

  return <div className="empty-app-state">{content}</div>;
}
