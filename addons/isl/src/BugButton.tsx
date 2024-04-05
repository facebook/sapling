/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Heartbeat} from './heartbeat';

import {Copyable} from './Copyable';
import {DropdownFields} from './DropdownFields';
import {ErrorBoundary, ErrorNotice} from './ErrorNotice';
import {Internal} from './Internal';
import {Tooltip} from './Tooltip';
import {Button} from './components/Button';
import {Divider} from './components/Divider';
import {DEFAULT_HEARTBEAT_TIMEOUT_MS, useHeartbeat} from './heartbeat';
import {t, T} from './i18n';
import platform from './platform';
import {applicationinfo} from './serverAPIState';
import * as stylex from '@stylexjs/stylex';
import {useAtomValue} from 'jotai';
import {Suspense} from 'react';
import {Icon} from 'shared/Icon';

import './BugButton.css';

const styles = stylex.create({
  centered: {
    justifyContent: 'center',
  },
});

export function BugButton() {
  return (
    <Tooltip
      trigger="click"
      component={dismiss => <BugDropdown dismiss={dismiss} />}
      group="topbar"
      placement="bottom">
      <Button icon data-testid="bug-button">
        <Icon icon="bug" />
      </Button>
    </Tooltip>
  );
}

function BugDropdown({dismiss}: {dismiss: () => void}) {
  const heartbeat = useHeartbeat();

  const AdditionalDebugContent = platform.AdditionalDebugContent;
  return (
    <DropdownFields
      title={<T>Help</T>}
      icon="bug"
      data-testid="bug-dropdown"
      className="bug-dropdown">
      <ISLVersion />
      <HeartbeatWarning heartbeat={heartbeat} />
      <div className="bug-dropdown-actions">
        <FileABug dismissBugDropdown={dismiss} heartbeat={heartbeat} />
        {AdditionalDebugContent && (
          <div className="additional-debug-content">
            <Divider />
            <ErrorBoundary>
              <Suspense>
                <AdditionalDebugContent />
              </Suspense>
            </ErrorBoundary>
          </div>
        )}
      </div>
    </DropdownFields>
  );
}

function ISLVersion() {
  const info = useAtomValue(applicationinfo);
  if (info == null) {
    return <Icon icon="loading" />;
  }

  return (
    <div className="bug-dropdown-version">
      <Copyable children={`ISL version ${info.version} (${info.platformName})`} />
    </div>
  );
}

function HeartbeatWarning({heartbeat}: {heartbeat: Heartbeat}) {
  const appInfo = useAtomValue(applicationinfo);
  if (heartbeat.type === 'timeout') {
    return (
      <>
        <ErrorNotice
          error={new Error(t(`Heartbeat timed out after ${DEFAULT_HEARTBEAT_TIMEOUT_MS}ms`))}
          title={t("Can't reach server — most features won't work")}
          description={t('The ISL server needs to be restarted')}></ErrorNotice>
        {appInfo && (
          <div>
            <T
              replace={{
                $logfile: (
                  <code>
                    <Copyable className="log-file-path">{appInfo.logFilePath}</Copyable>
                  </code>
                ),
              }}>
              Your log file is located at: $logfile
            </T>
          </div>
        )}
      </>
    );
  }
  return null;
}

function FileABug({
  dismissBugDropdown,
  heartbeat,
}: {
  dismissBugDropdown: () => void;
  heartbeat: Heartbeat;
}) {
  return Internal.FileABugButton != null ? (
    <Internal.FileABugButton dismissBugDropdown={dismissBugDropdown} heartbeat={heartbeat} />
  ) : (
    <OSSFileABug />
  );
}

function OSSFileABug() {
  return (
    <>
      <Button
        xstyle={styles.centered}
        onClick={() => {
          platform.openExternalLink('https://sapling-scm.com/docs/addons/isl');
        }}>
        <Icon icon="book" slot="start" />
        <T>View Documentation</T>
      </Button>
      <Button
        xstyle={styles.centered}
        onClick={() => {
          platform.openExternalLink('https://discord.gg/X6baZ94Vzh');
        }}>
        <Icon icon="comment-discussion" slot="start" />
        <T>Help and Feedback on Discord</T>
      </Button>
      <Button
        xstyle={styles.centered}
        onClick={() => {
          platform.openExternalLink('https://github.com/facebook/sapling/issues');
        }}>
        <Icon icon="bug" slot="start" />
        <T>Report an Issue on GitHub</T>
      </Button>
    </>
  );
}
