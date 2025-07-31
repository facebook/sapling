/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {DropdownFields} from './DropdownFields';
import {t, T} from './i18n';
import type {CommitInfo} from './types';

import {useAtomValue, useSetAtom} from 'jotai';
import {tracker} from './analytics';
import {uncommittedChangesWithPreviews} from './previews';
import './SmartActionsMenu.css';
import {useConfirmUnsavedEditsBeforeSplit} from './stackEdit/ui/ConfirmUnsavedEditsBeforeSplit';
import {editingStackIntentionHashes, shouldAutoSplitState} from './stackEdit/ui/stackEditState';

export function SmartActionsMenu({commit}: {commit: CommitInfo}) {
  return (
    <Tooltip
      component={dismiss => <SmartActions commit={commit} dismiss={dismiss} />}
      trigger="click"
      title={<T>Smart Actions...</T>}>
      <Button icon data-testid="smart-actions-button" className="smart-actions-button">
        <Icon icon="lightbulb" />
      </Button>
    </Tooltip>
  );
}

function SmartActions({commit, dismiss}: {commit: CommitInfo; dismiss: () => void}) {
  return (
    <DropdownFields
      title={<T>Smart Actions</T>}
      icon="lightbulb"
      className="smart-actions-dropdown"
      data-testid="smart-actions-dropdown">
      <AutoSplitButton commit={commit} dismiss={dismiss} />
    </DropdownFields>
  );
}

/** Like SplitButton, but triggers AI split automatically. */
export function AutoSplitButton({
  commit,
  dismiss,
}: {commit: CommitInfo; dismiss: () => void} & React.ComponentProps<typeof Button>) {
  const confirmUnsavedEditsBeforeSplit = useConfirmUnsavedEditsBeforeSplit();
  const setEditStackIntentionHashes = useSetAtom(editingStackIntentionHashes);
  const setShouldAutoSplit = useSetAtom(shouldAutoSplitState);

  const uncommittedChanges = useAtomValue(uncommittedChangesWithPreviews);
  const hasUncommittedChanges = uncommittedChanges.length > 0;

  return (
    <Tooltip
      title={hasUncommittedChanges ? t('Cannot currently split with uncommitted changes') : ''}
      trigger={hasUncommittedChanges ? 'hover' : 'disabled'}>
      <Button
        icon
        onClick={async e => {
          if (!(await confirmUnsavedEditsBeforeSplit([commit], 'split'))) {
            return;
          }
          setEditStackIntentionHashes(['split', new Set([commit.hash])]);
          setShouldAutoSplit(true);
          tracker.track('SplitOpenFromSmartActions');
          dismiss();
          e.stopPropagation();
        }}
        disabled={hasUncommittedChanges}>
        <T>Auto-split with AI</T>
      </Button>
    </Tooltip>
  );
}
