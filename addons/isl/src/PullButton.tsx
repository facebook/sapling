/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Operation} from './operations/Operation';

import {Internal} from './Internal';
import {DOCUMENTATION_DELAY, Tooltip} from './Tooltip';
import {VSCodeButtonDropdown} from './VSCodeButtonDropdown';
import {t, T} from './i18n';
import {PullOperation} from './operations/PullOperation';
import {persistAtomToConfigEffect} from './persistAtomToConfigEffect';
import {uncommittedChangesWithPreviews, useMostRecentPendingOperation} from './previews';
import {relativeDate, RelativeDate} from './relativeDate';
import {latestCommits, useRunOperation} from './serverAPIState';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {atom, useRecoilState, useRecoilValue} from 'recoil';
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
const pullButtonChoiceKey = atom<string>({
  key: 'pullButtonChoiceKey',
  default: DEFAULT_PULL_BUTTON.id,
  effects: [persistAtomToConfigEffect('isl.pull-button-choice')],
});

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
  // no need to use previews here, we only need the latest commits to find the last pull timestamp.
  const commits = useRecoilValue(latestCommits);
  // assuming master is getting updated frequently, last pull time should equal the newest commit in the history.
  const lastSync =
    commits.length === 0 ? null : Math.max(...commits.map(info => info.date.valueOf()));

  const pullButtonOptions: Array<PullButtonOption> = [];
  pullButtonOptions.push(DEFAULT_PULL_BUTTON, ...(Internal.additionalPullOptions ?? []));

  const [dropdownChoiceKey, setDropdownChoiceKey] = useRecoilState(pullButtonChoiceKey);
  const currentChoice =
    pullButtonOptions.find(option => option.id === dropdownChoiceKey) ?? pullButtonOptions[0];

  const trackedChanges = useRecoilValue(uncommittedChangesWithPreviews).filter(
    change => change.status !== '?',
  );
  const hasUncommittedChnages = trackedChanges.length > 0;

  const disabledFromUncommittedChanges =
    currentChoice.allowWithUncommittedChanges === false && hasUncommittedChnages;

  let tooltip =
    currentChoice.tooltip +
    (lastSync == null
      ? ''
      : '\n\n' +
        t('Latest fetched commit is $date old', {
          replace: {$date: relativeDate(lastSync, {useRelativeForm: true})},
        })) +
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
            onClick={() => runOperation(currentChoice.getOperation())}
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
            }}>
            <Icon slot="start" icon={isRunningPull ? 'loading' : 'cloud-download'} />
            <T>Pull</T>
          </VSCodeButton>
        )}
        {lastSync && <RelativeDate date={lastSync} useShortVariant />}
      </div>
    </Tooltip>
  );
}
