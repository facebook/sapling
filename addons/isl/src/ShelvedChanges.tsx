/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ShelvedChange} from './types';

import serverAPI from './ClientToServerAPI';
import {OpenComparisonViewButton} from './ComparisonView/OpenComparisonViewButton';
import {FlexSpacer} from './ComponentUtils';
import {DropdownFields} from './DropdownFields';
import {EmptyState} from './EmptyState';
import {ErrorNotice} from './ErrorNotice';
import {useCommandEvent} from './ISLShortcuts';
import {Kbd} from './Kbd';
import {OperationDisabledButton} from './OperationDisabledButton';
import {Subtle} from './Subtle';
import {Tooltip} from './Tooltip';
import {ChangedFiles} from './UncommittedChanges';
import {T, t} from './i18n';
import {UnshelveOperation} from './operations/UnshelveOperation';
import {RelativeDate} from './relativeDate';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useEffect} from 'react';
import {selector, useRecoilValueLoadable, useRecoilRefresher_UNSTABLE} from 'recoil';
import {ComparisonType} from 'shared/Comparison';
import {Icon} from 'shared/Icon';
import {KeyCode, Modifier} from 'shared/KeyboardShortcuts';

import './ShelvedChanges.css';

const shelvedChangesState = selector<Array<ShelvedChange>>({
  key: 'shelvedChangesState',
  get: async () => {
    serverAPI.postMessage({
      type: 'fetchShelvedChanges',
    });

    const result = await serverAPI.nextMessageMatching('fetchedShelvedChanges', () => true);
    if (result.shelvedChanges.error != null) {
      throw new Error(result.shelvedChanges.error.toString());
    }
    return result.shelvedChanges.value;
  },
});

export function ShelvedChangesMenu() {
  const additionalToggles = useCommandEvent('ToggleShelvedChangesDropdown');
  return (
    <Tooltip
      component={dismiss => <ShelvedChangesList dismiss={dismiss} />}
      trigger="click"
      placement="bottom"
      additionalToggles={additionalToggles}
      title={
        <T replace={{$shortcut: <Kbd keycode={KeyCode.S} modifiers={[Modifier.ALT]} />}}>
          Shelved Changes ($shortcut)
        </T>
      }>
      <VSCodeButton appearance="icon" data-testid="shelved-changes-button">
        <Icon icon="archive" />
      </VSCodeButton>
    </Tooltip>
  );
}

function ShelvedChangesList({dismiss}: {dismiss: () => void}) {
  const shelvedChanges = useRecoilValueLoadable(shelvedChangesState);
  const refresh = useRecoilRefresher_UNSTABLE(shelvedChangesState);
  useEffect(() => {
    // make sure we fetch whenever loading the shelved changes list
    refresh();
  }, [refresh]);
  return (
    <DropdownFields
      title={
        <span className="shelved-changes-title">
          <T>Shelved Changes</T>{' '}
          <Tooltip
            title={t(
              'You can Shelve a set of uncommitted changes to save them for later, via the Shelve button in the list of uncommitted changes.\n\nHere you can view and Unshelve previously shelved changes.',
            )}>
            <Icon icon="info" />
          </Tooltip>
        </span>
      }
      icon="archive"
      className="shelved-changes-dropdown"
      data-testid="shelved-changes-dropdown">
      {shelvedChanges.state === 'loading' ? (
        <Icon icon="loading" />
      ) : shelvedChanges.state === 'hasError' ? (
        <ErrorNotice
          title="Could not fetch shelved changes"
          error={shelvedChanges.errorOrThrow()}
        />
      ) : shelvedChanges.valueOrThrow().length === 0 ? (
        <EmptyState small>
          <T>No shelved changes</T>
        </EmptyState>
      ) : (
        <div className="shelved-changes-list">
          {shelvedChanges.valueOrThrow().map(change => {
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
                  <OperationDisabledButton
                    appearance="secondary"
                    contextKey={`unshelve-${change.hash}`}
                    className="unshelve-button"
                    runOperation={() => {
                      dismiss();
                      return new UnshelveOperation(change);
                    }}
                    icon={<Icon icon="layers-active" slot="start" />}>
                    <T>Unshelve</T>
                  </OperationDisabledButton>
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
