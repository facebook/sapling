/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {MessageBusStatus} from './MessageBus';
import type {RepoInfo} from './types';
import type {TrackErrorName} from 'isl-server/src/analytics/eventNames';
import type {ReactNode} from 'react';

import {ErrorNotice} from './ErrorNotice';
import {Internal} from './Internal';
import {tracker} from './analytics';
import {allDiffSummaries} from './codeReview/CodeReviewInfo';
import {t, T} from './i18n';
import platform from './platform';
import {reconnectingStatus, repositoryInfo} from './serverAPIState';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useRecoilValue} from 'recoil';
import {useThrottledEffect} from 'shared/hooks';

type TopLevelErrorInfo = {
  title: ReactNode;
  error: Error;
  buttons?: Array<ReactNode>;
  trackErrorName?: TrackErrorName;
};

function computeTopLevelError(
  repoInfo: RepoInfo | undefined,
  reconnectStatus: MessageBusStatus,
  diffFetchError: Error | undefined,
): TopLevelErrorInfo | undefined {
  if (reconnectStatus.type === 'reconnecting') {
    return {
      title: <T>Connection to server was lost</T>,
      error: new Error(t('Attempting to reconnect...')),
    };
  } else if (reconnectStatus.type === 'error') {
    if (reconnectStatus.error === 'Invalid token') {
      return {
        title: (
          <T>
            Unable to connect to server. Try closing this window and accessing ISL with a fresh
            link.
          </T>
        ),
        error: new Error(
          t(
            'Invalid connection token. ' +
              'For security, you need to open a new ISL window when the server is restarted.',
          ),
        ),
      };
    }
    return {
      title: <T>Error connecting to server</T>,
      error: new Error(reconnectStatus.error),
      // no use tracking, since we can't reach the server anyway.
    };
  } else if (diffFetchError) {
    const internalResult = Internal.findInternalError?.(diffFetchError) as
      | TopLevelErrorInfo
      | undefined;
    if (internalResult != null) {
      return internalResult;
    } else if (repoInfo?.type === 'success' && repoInfo.codeReviewSystem.type === 'github') {
      const learnAboutGhButton = (
        <VSCodeButton
          appearance="secondary"
          onClick={e => {
            platform.openExternalLink('https://sapling-scm.com/docs/git/intro');
            e.preventDefault();
            e.stopPropagation();
          }}>
          <T>Learn more</T>
        </VSCodeButton>
      );
      if (diffFetchError.message.startsWith('NotAuthenticatedError')) {
        const error = new Error(
          t('Log in to gh CLI with `gh auth login` to allow requests to GitHub'),
        );
        error.stack = diffFetchError.stack;
        return {
          title: <T>Not Authenticated to GitHub with `gh` CLI</T>,
          error,
          buttons: [learnAboutGhButton],
          trackErrorName: 'GhCliNotAuthenticated',
        };
      } else if (diffFetchError.message.startsWith('GhNotInstalledError')) {
        const error = new Error(t('Install the `gh` CLI to make requests to GitHub'));
        error.stack = diffFetchError.stack;
        return {
          title: <T>Unable to fetch data from Github</T>,
          error,
          buttons: [learnAboutGhButton],
          trackErrorName: 'GhCliNotInstalled',
        };
      }
    }
    return {
      title: <T>Failed to fetch Diffs</T>,
      error: diffFetchError,
      trackErrorName: 'DiffFetchFailed',
    };
  }

  return undefined;
}

export function TopLevelErrors() {
  const reconnectStatus = useRecoilValue(reconnectingStatus);
  const repoInfo = useRecoilValue(repositoryInfo);
  const diffFetchError = useRecoilValue(allDiffSummaries).error;

  const info = computeTopLevelError(repoInfo, reconnectStatus, diffFetchError);

  if (info == null) {
    return null;
  }

  return <TrackedError info={info} />;
}

function TrackedError({info}: {info: TopLevelErrorInfo}) {
  useThrottledEffect(
    () => {
      if (info.trackErrorName != null) {
        tracker.error('TopLevelErrorShown', info.trackErrorName, info.error);
      }
    },
    1_000,
    [info.trackErrorName, info.error],
  );
  return <ErrorNotice title={info.title} error={info.error} buttons={info.buttons} />;
}
