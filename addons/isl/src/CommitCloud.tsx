/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitCloudSyncState, Hash, Result} from './types';

import serverAPI from './ClientToServerAPI';
import {Commit} from './Commit';
import {FlexSpacer} from './ComponentUtils';
import {ErrorNotice} from './ErrorNotice';
import {OperationDisabledButton} from './OperationDisabledButton';
import {Tooltip} from './Tooltip';
import {T, t} from './i18n';
import {CommitCloudSyncOperation} from './operations/CommitCloudSyncOperation';
import {CommitPreview, treeWithPreviews} from './previews';
import {RelativeDate} from './relativeDate';
import {CommitCloudBackupStatus} from './types';
import {VSCodeButton, VSCodeDropdown, VSCodeOption} from '@vscode/webview-ui-toolkit/react';
import {useEffect} from 'react';
import {atom, useRecoilValue} from 'recoil';
import {Icon} from 'shared/Icon';
import {notEmpty} from 'shared/utils';

import './CommitCloud.css';

const cloudSyncStateAtom = atom<Result<CommitCloudSyncState> | null>({
  key: 'cloudSyncStateAtom',
  default: null,
  effects: [
    ({setSelf}) => {
      const disposable = serverAPI.onMessageOfType('fetchedCommitCloudState', event => {
        setSelf(event.state);
      });
      return () => disposable.dispose();
    },
    () =>
      serverAPI.onSetup(() =>
        serverAPI.postMessage({
          type: 'fetchCommitCloudState',
        }),
      ),
  ],
});

const MIN_TIME_TO_RECHECK_MS = 10 * 1000;

export function CommitCloudInfo() {
  const cloudSyncState = useRecoilValue(cloudSyncStateAtom);

  useEffect(() => {
    if (
      cloudSyncState?.value?.lastChecked &&
      Date.now() - cloudSyncState?.value?.lastChecked.valueOf() > MIN_TIME_TO_RECHECK_MS
    ) {
      serverAPI.postMessage({
        type: 'fetchCommitCloudState',
      });
    }
  }, [cloudSyncState]);

  return (
    <div className="commit-cloud-info">
      <div className="dropdown-fields-header commit-cloud-header">
        <Icon icon="cloud" size="M" />
        <strong role="heading">{<T>Commit Cloud</T>}</strong>
        <Tooltip
          title={t(
            'Commit Cloud backs up your draft commits automatically across all your devices.',
          )}>
          <Icon icon="info" />
        </Tooltip>
      </div>
      {cloudSyncState?.value?.commitStatuses == null ? null : (
        <CommitCloudSyncStatusBadge statuses={cloudSyncState?.value?.commitStatuses} />
      )}
      <div className="commit-cloud-row">
        {cloudSyncState == null ? (
          <Icon icon="loading" />
        ) : cloudSyncState.error != null ? (
          <ErrorNotice
            error={cloudSyncState.error}
            title={t('Failed to check Commit Cloud state')}
          />
        ) : cloudSyncState.value.lastBackup == null ? null : (
          <T
            replace={{
              $relTimeAgo: (
                <Tooltip title={cloudSyncState.value.lastBackup.toLocaleString()}>
                  <RelativeDate date={cloudSyncState.value.lastBackup} />
                </Tooltip>
              ),
            }}>
            Last meaningful sync: $relTimeAgo
          </T>
        )}
        <FlexSpacer />
        <OperationDisabledButton
          contextKey="cloud-sync"
          runOperation={() => {
            return new CommitCloudSyncOperation();
          }}
          appearance="secondary">
          <T>Sync now</T>
        </OperationDisabledButton>
      </div>

      <div className="commit-cloud-row">
        {cloudSyncState?.value == null ? null : (
          <div className="commit-cloud-dropdown-container">
            <label htmlFor="stack-file-dropdown">
              <T>Current Workspace</T>
            </label>
            <VSCodeDropdown
              value={cloudSyncState?.value.currentWorkspace}
              onChange={event => {
                // TODO
              }}>
              {cloudSyncState?.value.workspaceChoices?.map(name => (
                <VSCodeOption key={name} value={name}>
                  {name}
                </VSCodeOption>
              ))}
            </VSCodeDropdown>
          </div>
        )}
      </div>
    </div>
  );
}

function CommitCloudSyncStatusBadge({statuses}: {statuses: Map<Hash, CommitCloudBackupStatus>}) {
  const statusValues = [...statuses.entries()];
  const pending = statusValues.filter(
    ([_hash, status]) =>
      status === CommitCloudBackupStatus.Pending || status === CommitCloudBackupStatus.InProgress,
  );
  const failed = statusValues.filter(
    ([_hash, status]) => status === CommitCloudBackupStatus.Failed,
  );

  let icon;
  let content;
  let renderTooltip;
  if (pending.length > 0) {
    icon = 'sync';
    content = <T count={pending.length}>commitsBeingBackedUp</T>;
    renderTooltip = () => <BackupList commits={pending.map(([hash]) => hash)} />;
  } else if (failed.length > 0) {
    icon = 'sync';
    content = (
      <div className="inline-error-badge">
        <span>
          <Icon icon="error" slot="start" />
          <T count={failed.length}>commitsFailedBackingUp</T>
        </span>
      </div>
    );
    renderTooltip = () => <BackupList commits={failed.map(([hash]) => hash)} />;
  } else if (statusValues.length > 0) {
    icon = 'check';
    content = <T>All commits backed up</T>;
  } else {
    icon = 'question';
    content = <T>No commits found</T>;
  }

  return (
    <div className="commit-cloud-row commit-cloud-sync-status-badge">
      {renderTooltip == null ? (
        <div>
          <Icon icon={icon} />
          {content}
        </div>
      ) : (
        <Tooltip component={renderTooltip}>
          <Icon icon={icon} />
          {content}
        </Tooltip>
      )}
    </div>
  );
}

function BackupList({commits}: {commits: Array<Hash>}) {
  const treeMap = useRecoilValue(treeWithPreviews).treeMap;
  const infos = commits.map(hash => treeMap.get(hash)?.info).filter(notEmpty);
  return (
    <div className="commit-cloud-backup-list">
      {infos.map(commit => (
        <Commit
          commit={commit}
          key={commit.hash}
          hasChildren={false}
          previewType={CommitPreview.NON_ACTIONABLE_COMMIT}
        />
      ))}
    </div>
  );
}
