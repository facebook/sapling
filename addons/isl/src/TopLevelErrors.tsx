/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {MessageBusStatus} from './MessageBus';

import {ErrorNotice} from './ErrorNotice';
import messageBus from './MessageBus';
import {allDiffSummaries, repositoryInfo} from './codeReview/CodeReviewInfo';
import {t, T} from './i18n';
import platform from './platform';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useEffect, useState} from 'react';
import {useRecoilValue} from 'recoil';

export function TopLevelErrors() {
  const reconnectingStatus = useReconnectingStatus();
  const repoInfo = useRecoilValue(repositoryInfo);

  const diffFetchError = useRecoilValue(allDiffSummaries).error;

  if (reconnectingStatus.type === 'reconnecting') {
    return (
      <ErrorNotice
        title={<T>Connection to server was lost</T>}
        error={new Error(t('Attempting to reconnect...'))}
      />
    );
  } else if (reconnectingStatus.type === 'error') {
    if (reconnectingStatus.error === 'Invalid token') {
      return (
        <ErrorNotice
          title={
            <T>
              Unable to connect to server. Try closing this window and accessing ISL with a fresh
              link.
            </T>
          }
          error={
            new Error(
              t(
                'Invalid connection token. For security, you need to open a new ISL window when the server is restarted.',
              ),
            )
          }
        />
      );
    }
    return (
      <ErrorNotice
        title={<T>Error connecting to server</T>}
        error={new Error(reconnectingStatus.error)}
      />
    );
  } else if (diffFetchError) {
    if (repoInfo?.codeReviewSystem.type === 'github') {
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
        const err = new Error(
          t('Log in to gh CLI with `gh auth login` to allow requests to GitHub'),
        );
        err.stack = diffFetchError.stack;
        return (
          <ErrorNotice
            title={<T>Not Authenticated to GitHub with `gh` CLI</T>}
            error={err}
            buttons={[learnAboutGhButton]}
          />
        );
      } else if (diffFetchError.message.startsWith('GhNotInstalledError')) {
        const err = new Error(t('Install the `gh` CLI to make requests to GitHub'));
        err.stack = diffFetchError.stack;
        return (
          <ErrorNotice
            title={<T>Unable to fetch data from Github</T>}
            error={err}
            buttons={[learnAboutGhButton]}
          />
        );
      }
    }
    return <ErrorNotice title={<T>Failed to fetch Diffs</T>} error={diffFetchError} />;
  }
  return null;
}

function useReconnectingStatus() {
  const [connectionStatus, setConnectionStatus] = useState<MessageBusStatus>({
    type: 'initializing',
  });
  useEffect(() => {
    const disposable = messageBus.onChangeStatus(setConnectionStatus);
    return () => disposable.dispose();
  }, []);
  return connectionStatus;
}
