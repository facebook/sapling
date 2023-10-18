/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitCloudSyncState, Result} from './types';

import serverAPI from './ClientToServerAPI';
import {FlexSpacer} from './ComponentUtils';
import {ErrorNotice} from './ErrorNotice';
import {Tooltip} from './Tooltip';
import {T, t} from './i18n';
import {RelativeDate} from './relativeDate';
import {VSCodeButton, VSCodeDropdown, VSCodeOption} from '@vscode/webview-ui-toolkit/react';
import {atom, useRecoilValue} from 'recoil';
import {Icon} from 'shared/Icon';

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

export function CommitCloudInfo() {
  const cloudSyncState = useRecoilValue(cloudSyncStateAtom);
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
      <div className="download-commits-input-row">
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
            Last backed up: $relTimeAgo
          </T>
        )}
        <FlexSpacer />
        <VSCodeButton appearance="secondary">
          <T>Sync now</T>
        </VSCodeButton>
      </div>

      <div className="download-commits-input-row">
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
