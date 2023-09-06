/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Result, ShelvedChange} from './types';

import serverAPI from './ClientToServerAPI';
import {OpenComparisonViewButton} from './ComparisonView/OpenComparisonViewButton';
import {FlexSpacer} from './ComponentUtils';
import {DropdownFields} from './DropdownFields';
import {EmptyState} from './EmptyState';
import {ErrorNotice} from './ErrorNotice';
import {Subtle} from './Subtle';
import {Tooltip} from './Tooltip';
import {ChangedFiles} from './UncommittedChanges';
import {T, t} from './i18n';
import {RelativeDate} from './relativeDate';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {atom, useRecoilValue} from 'recoil';
import {ComparisonType} from 'shared/Comparison';
import {Icon} from 'shared/Icon';

import './ShelvedChanges.css';

const shelvedChangesState = atom<Result<Array<ShelvedChange>>>({
  key: 'shelvedChangesState',
  default: {value: []},
  effects: [
    ({setSelf}) => {
      const disposable = serverAPI.onMessageOfType('fetchedShelvedChanges', event => {
        setSelf(event.shelvedChanges);
      });
      return () => disposable.dispose();
    },
    () =>
      serverAPI.onSetup(() =>
        serverAPI.postMessage({
          type: 'fetchShelvedChanges',
        }),
      ),
  ],
});

export function ShelvedChangesMenu() {
  return (
    <Tooltip
      component={dismiss => <ShelvedChangesList dismiss={dismiss} />}
      trigger="click"
      placement="bottom"
      title={t('Shelved Changes')}>
      <VSCodeButton appearance="icon" data-testid="shelved-changes-button">
        <Icon icon="archive" />
      </VSCodeButton>
    </Tooltip>
  );
}

export function ShelvedChangesList({dismiss}: {dismiss: () => void}) {
  const shelvedChanges = useRecoilValue(shelvedChangesState);
  return (
    <DropdownFields
      title={<T>Shelved Changes</T>}
      icon="archive"
      className="shelved-changes-dropdown"
      data-testid="shelved-changes-dropdown">
      {shelvedChanges.error ? (
        <ErrorNotice title="Could not fetch shelved changes" error={shelvedChanges.error} />
      ) : shelvedChanges.value.length === 0 ? (
        <EmptyState small>
          <T>No shelved changes</T>
        </EmptyState>
      ) : (
        <div className="shelved-changes-list">
          {shelvedChanges.value.map(change => {
            const comparison = {
              type: ComparisonType.Committed,
              hash: change.hash,
            };
            return (
              <div key={change.hash} className="shelved-changes-item">
                <div className="shelved-changes-item-row">
                  <span className="shelve-name">{change.name}</span>
                  <Subtle>
                    <RelativeDate date={change.date} useShortVariant />
                  </Subtle>
                  <FlexSpacer />
                  <VSCodeButton
                    appearance="secondary"
                    className="unshelve-button"
                    onClick={() => {
                      /* TODO: run unshelve */
                    }}>
                    <Icon icon="layers-active" slot="start" />
                    <T>Unshelve</T>
                  </VSCodeButton>
                </div>
                <OpenComparisonViewButton
                  comparison={comparison}
                  buttonText={<T>View Changes</T>}
                  onClick={dismiss}
                />
                <div className="shelved-changes-item-row">
                  <ChangedFiles
                    filesSubset={change.filesSample}
                    totalFiles={change.totalFileCount}
                    comparison={comparison}
                  />
                </div>
              </div>
            );
          })}
        </div>
      )}
    </DropdownFields>
  );
}
