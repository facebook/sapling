/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {AppMode, RepositoryError} from './types';

import {Button} from 'isl-components/Button';
import {ErrorBoundary, ErrorNotice} from 'isl-components/ErrorNotice';
import {Icon} from 'isl-components/Icon';
import {atom, useAtomValue, useSetAtom} from 'jotai';
import {Suspense, useEffect, useMemo} from 'react';
import {useThrottledEffect} from 'shared/hooks';
import {AllProviders} from './AppWrapper';
import {CommandHistoryAndProgress} from './CommandHistoryAndProgress';
import {CommitInfoSidebar} from './CommitInfoView/CommitInfoView';
import {CommitTreeList} from './CommitTreeList';
import {ComparisonViewApp, ComparisonViewModal} from './ComparisonView/ComparisonViewModal';
import {availableCwds, CwdSelections} from './CwdSelector';
import {Drawers} from './Drawers';
import {EmptyState} from './EmptyState';
import {useCommand} from './ISLShortcuts';
import {PRDashboard} from './PRDashboard';
import {Confetti} from './Confetti';
import {Internal} from './Internal';
import {TopBar} from './TopBar';
import {TopLevelAlerts} from './TopLevelAlert';
import {TopLevelErrors} from './TopLevelErrors';
import {tracker} from './analytics';
import {islDrawerState} from './drawerState';
import {t, T} from './i18n';
import platform from './platform';
import {useMainContentWidth} from './responsive';
import {repositoryInfoOrError} from './serverAPIState';

import clientToServerAPI from './ClientToServerAPI';
import './index.css';
import './stackEdit/ui/AISplitMessageHandlers';

declare global {
  interface Window {
    /**
     * AppMode that determines what feature the App is rendering.
     * This is set at creation time (e.g. in HTML), and is not dynamic.
     */
    islAppMode?: AppMode;
  }
}
let hasInitialized = false;
export default function App() {
  const mode = window.islAppMode ?? {mode: 'isl'};

  useEffect(() => {
    if (!hasInitialized) {
      clientToServerAPI.postMessage({type: 'clientReady'});
      hasInitialized = true;
    }
  }, []);

  return (
    <AllProviders>
      {mode.mode === 'isl' ? (
        <>
          <NullStateOrDrawers />
          <ComparisonViewModal />
        </>
      ) : (
        <ComparisonApp />
      )}
    </AllProviders>
  );
}

function ComparisonApp() {
  return (
    <Suspense fallback={<Icon icon="loading" />}>
      <ComparisonViewApp />
    </Suspense>
  );
}

function NullStateOrDrawers() {
  const repoInfo = useAtomValue(repositoryInfoOrError);
  if (repoInfo != null && repoInfo.type !== 'success') {
    return <ISLNullState repoError={repoInfo} />;
  }
  return <ISLDrawers />;
}

function ISLDrawers() {
  const setDrawerState = useSetAtom(islDrawerState);
  useCommand('ToggleSidebar', () => {
    setDrawerState(state => ({
      ...state,
      right: {...state.right, collapsed: !state.right.collapsed},
    }));
  });
  useCommand('ToggleLeftSidebar', () => {
    setDrawerState(state => ({
      ...state,
      left: {...state.left, collapsed: !state.left.collapsed},
    }));
  });

  return (
    <Drawers
      leftLabel={
        <>
          <Icon icon="git-pull-request" />
          <T>PR Stacks</T>
        </>
      }
      left={<PRDashboard />}
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
      <Confetti />
    </Drawers>
  );
}

function MainContent() {
  const ref = useMainContentWidth();
  return (
    <div className="main-content-area" ref={ref}>
      <TopBar />
      <TopLevelErrors />
      <TopLevelAlerts />
      <CommitTreeList />
    </div>
  );
}

function ISLNullState({repoError}: {repoError: RepositoryError}) {
  const emptyCwds = useAtomValue(useMemo(() => atom(get => get(availableCwds).length === 0), []));
  useThrottledEffect(
    () => {
      if (repoError != null) {
        switch (repoError.type) {
          case 'cwdNotARepository':
            tracker.track('UIEmptyState', {extras: {cwd: repoError.cwd}, errorName: 'InvalidCwd'});
            break;
          case 'edenFsUnhealthy':
            tracker.track('UIEmptyState', {
              extras: {cwd: repoError.cwd},
              errorName: 'EdenFsUnhealthy',
            });
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
      if (platform.platformName === 'vscode' && emptyCwds) {
        content = (
          <>
            <EmptyState>
              <div>
                <T>No folder opened</T>
              </div>
              <p>
                <T>Open a folder to get started.</T>
              </p>
            </EmptyState>
          </>
        );
      } else {
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
      }
    } else if (repoError.type === 'cwdDoesNotExist') {
      content = (
        <>
          {Internal.InternalInstallationDocs ? (
            <Internal.InternalInstallationDocs repoError={repoError} />
          ) : (
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
                <Button
                  key="help-button"
                  onClick={e => {
                    platform.openExternalLink(
                      'https://sapling-scm.com/docs/introduction/installation',
                    );
                    e.preventDefault();
                    e.stopPropagation();
                  }}>
                  <T>See installation docs</T>
                </Button>,
              ]}
            />
          )}
          <CwdSelections dismiss={() => null} />
        </>
      );
    } else if (repoError.type === 'edenFsUnhealthy') {
      content = (
        <>
          <ErrorNotice
            title={<T replace={{$cwd: repoError.cwd}}>EdenFS is not running properly in $cwd</T>}
            description={
              <T replace={{$edenDoctor: <code>eden doctor</code>}}>
                Try running $edenDoctor and reloading the ISL window
              </T>
            }
            error={
              new Error(
                t('README_EDEN.txt found in $cwd', {
                  replace: {$cwd: repoError.cwd},
                }),
              )
            }
          />
          <CwdSelections dismiss={() => null} />
        </>
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
              <Button
                key="help-button"
                onClick={e => {
                  platform.openExternalLink(
                    'https://sapling-scm.com/docs/introduction/installation',
                  );
                  e.preventDefault();
                  e.stopPropagation();
                }}>
                <T>See installation docs</T>
              </Button>,
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
