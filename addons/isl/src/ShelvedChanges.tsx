/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Result, ShelvedChange} from './types';

import serverAPI from './ClientToServerAPI';
import {DropdownFields} from './DropdownFields';
import {EmptyState} from './EmptyState';
import {ErrorNotice} from './ErrorNotice';
import {Subtle} from './Subtle';
import {Tooltip} from './Tooltip';
import {T, t} from './i18n';
import {RelativeDate} from './relativeDate';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {atom, useRecoilValue} from 'recoil';
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
      component={() => <ShelvedChangesList />}
      trigger="click"
      placement="bottom"
      title={t('Shelved Changes')}>
      <VSCodeButton appearance="icon" data-testid="shelved-changes-button">
        <Icon icon="archive" />
      </VSCodeButton>
    </Tooltip>
  );
}

export function ShelvedChangesList() {
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
          {shelvedChanges.value.map(change => (
            <span key={change.hash} className="shelved-changes-item">
              <span>{change.name}</span>

              <Subtle>
                <RelativeDate date={change.date} useShortVariant />
              </Subtle>
            </span>
          ))}
        </div>
      )}
    </DropdownFields>
  );
}
