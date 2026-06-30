/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Operation} from './operations/Operation';

import {Button} from 'isl-components/Button';
import {ButtonDropdown} from 'isl-components/ButtonDropdown';
import {Icon} from 'isl-components/Icon';
import {Kbd} from 'isl-components/Kbd';
import {DOCUMENTATION_DELAY, Tooltip} from 'isl-components/Tooltip';
import {useAtom, useAtomValue} from 'jotai';
import {useCallback} from 'react';
import {fetchStableLocations} from './BookmarksData';
import {t, T} from './i18n';
import {Internal} from './Internal';
import {effectiveCommandsAtom, useCommand, useKeyboardShortcutsEnabled} from './ISLShortcuts';
import {configBackedAtom} from './jotaiUtils';
import {PullOperation} from './operations/PullOperation';
import {useRunOperation} from './operationsState';
import {uncommittedChangesWithPreviews, useMostRecentPendingOperation} from './previews';

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
  const hasUncommittedChanges = trackedChanges.length > 0;

  const disabledFromUncommittedChanges =
    currentChoice.allowWithUncommittedChanges === false && hasUncommittedChanges;

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

  // The shortcuts (and the rebind settings UI + tooltip hint) are gated behind a Gatekeeper
  // killswitch so the whole feature can be disabled remotely if it misbehaves.
  const keyboardShortcutsEnabled = useKeyboardShortcutsEnabled();

  const arcPullOption = pullButtonOptions.find(option => option.id === 'arc pull');
  const runPullOption = useCallback(
    (option: PullButtonOption | undefined) => {
      if (option == null) {
        return; // Arc Pull is absent in OSS builds — no-op.
      }
      const optionDisabled =
        (option.allowWithUncommittedChanges === false && hasUncommittedChanges) ||
        (pendingOperation != null && option.isRunning(pendingOperation));
      if (optionDisabled) {
        return;
      }
      runOperation(option.getOperation());
      fetchStableLocations();
    },
    [hasUncommittedChanges, pendingOperation, runOperation],
  );
  const handlePull = useCallback(() => {
    if (keyboardShortcutsEnabled) {
      runPullOption(DEFAULT_PULL_BUTTON);
    }
  }, [keyboardShortcutsEnabled, runPullOption]);
  const handleArcPull = useCallback(() => {
    if (keyboardShortcutsEnabled) {
      runPullOption(arcPullOption);
    }
  }, [keyboardShortcutsEnabled, arcPullOption, runPullOption]);
  useCommand('Pull', handlePull);
  useCommand('ArcPull', handleArcPull);

  // Hint the (override-aware) keyboard shortcut for the currently-selected choice in the tooltip.
  const effectiveCommands = useAtomValue(effectiveCommandsAtom);
  const [shortcutModifiers, shortcutKeyCode] =
    effectiveCommands[currentChoice.id === 'arc pull' ? 'ArcPull' : 'Pull'];
  const title = keyboardShortcutsEnabled ? (
    <div className="pull-tooltip">
      <div className="pull-tooltip-text">{tooltip}</div>
      <div className="pull-tooltip-shortcut">
        <T>Shortcut:</T>{' '}
        <Kbd
          modifiers={Array.isArray(shortcutModifiers) ? shortcutModifiers : [shortcutModifiers]}
          keycode={shortcutKeyCode}
        />
      </div>
    </div>
  ) : (
    tooltip
  );

  return (
    <Tooltip placement="bottom" delayMs={DOCUMENTATION_DELAY} title={title}>
      <div className="pull-info">
        {pullButtonOptions.length > 1 ? (
          <ButtonDropdown
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
          <Button
            disabled={!!isRunningPull}
            onClick={() => {
              runOperation(new PullOperation());
              fetchStableLocations();
            }}>
            <Icon slot="start" icon={isRunningPull ? 'loading' : 'cloud-download'} />
            <T>Pull</T>
          </Button>
        )}
      </div>
    </Tooltip>
  );
}
