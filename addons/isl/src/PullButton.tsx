/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Operation} from './operations/Operation';

import {fetchStableLocations} from './BookmarksData';
import {Internal} from './Internal';
import {DOCUMENTATION_DELAY, Tooltip} from './Tooltip';
import {VSCodeButtonDropdown} from './VSCodeButtonDropdown';
import {t, T} from './i18n';
import {configBackedAtom} from './jotaiUtils';
import {PullOperation} from './operations/PullOperation';
import {useRunOperation} from './operationsState';
import {uncommittedChangesWithPreviews, useMostRecentPendingOperation} from './previews';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useAtom, useAtomValue} from 'jotai';
import {Icon} from 'shared/Icon';

import './PullButton.css';

const DEFAULT_PULL_BUTTON = {
  id: 'pull',
  label: <T>Pull</T>,
  getOperation: () => new PullOperation(),
  isRunning: (op: Operation) => op instanceof PullOperation,
  tooltip: t('Fetch latest repository and branch information from remote.'),
  allowWithUncommittedChanges: true,
};
const pullButtonChoiceKey = configBackedAtom<string>(
  'isl.pull-button-choice',
  DEFAULT_PULL_BUTTON.id,
);

export type PullButtonOption = {
  id: string;
  label: React.ReactNode;
  getOperation: () => Operation;
  isRunning: (op: Operation) => boolean;
  tooltip: string;
  allowWithUncommittedChanges: boolean;
};

export function PullButton() {
  const runOperation = useRunOperation();

  const pullButtonOptions: Array<PullButtonOption> = [];
  pullButtonOptions.push(DEFAULT_PULL_BUTTON, ...(Internal.additionalPullOptions ?? []));

  const [dropdownChoiceKey, setDropdownChoiceKey] = useAtom(pullButtonChoiceKey);
  const currentChoice =
    pullButtonOptions.find(option => option.id === dropdownChoiceKey) ?? pullButtonOptions[0];

  const trackedChanges = useAtomValue(uncommittedChangesWithPreviews).filter(
    change => change.status !== '?',
  );
  const hasUncommittedChnages = trackedChanges.length > 0;

  const disabledFromUncommittedChanges =
    currentChoice.allowWithUncommittedChanges === false && hasUncommittedChnages;

  let tooltip =
    currentChoice.tooltip +
    (disabledFromUncommittedChanges == false
      ? ''
      : '\n\n' + t('Disabled due to uncommitted changes.'));
  const pendingOperation = useMostRecentPendingOperation();
  const isRunningPull = pendingOperation != null && currentChoice.isRunning(pendingOperation);
  if (isRunningPull) {
    tooltip += '\n\n' + t('Pull is already running.');
  }

  return (
    <Tooltip placement="bottom" delayMs={DOCUMENTATION_DELAY} title={tooltip}>
      <div className="pull-info">
        {pullButtonOptions.length > 1 ? (
          <VSCodeButtonDropdown
            appearance="secondary"
            buttonDisabled={!!isRunningPull || disabledFromUncommittedChanges}
            options={pullButtonOptions}
            onClick={() => {
              runOperation(currentChoice.getOperation());
              fetchStableLocations();
            }}
            onChangeSelected={choice => setDropdownChoiceKey(choice.id)}
            selected={currentChoice}
            icon={<Icon slot="start" icon={isRunningPull ? 'loading' : 'repo'} />}
          />
        ) : (
          <VSCodeButton
            appearance="secondary"
            disabled={!!isRunningPull}
            onClick={() => {
              runOperation(new PullOperation());
              fetchStableLocations();
            }}>
            <Icon slot="start" icon={isRunningPull ? 'loading' : 'cloud-download'} />
            <T>Pull</T>
          </VSCodeButton>
        )}
      </div>
    </Tooltip>
  );
}
