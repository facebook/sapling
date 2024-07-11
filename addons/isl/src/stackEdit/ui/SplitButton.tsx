/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from '../../types';
import type {TrackEventName} from 'isl-server/src/analytics/eventNames';

import {tracker} from '../../analytics';
import {T, t} from '../../i18n';
import {SplitCommitIcon} from '../../icons/SplitCommitIcon';
import {uncommittedChangesWithPreviews} from '../../previews';
import {useConfirmUnsavedEditsBeforeSplit} from './ConfirmUnsavedEditsBeforeSplit';
import {bumpStackEditMetric, editingStackIntentionHashes} from './stackEditState';
import {Button} from 'isl-components/Button';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtomValue, useSetAtom} from 'jotai';

/** Button to open split UI for the current commit. Expected to be shown on the head commit.
 * Loads that one commit in the split UI. */
export function SplitButton({
  commit,
  trackerEventName,
  ...rest
}: {commit: CommitInfo; trackerEventName: TrackEventName} & React.ComponentProps<typeof Button>) {
  const confirmUnsavedEditsBeforeSplit = useConfirmUnsavedEditsBeforeSplit();
  const setEditStackIntentionHashes = useSetAtom(editingStackIntentionHashes);

  const uncommittedChanges = useAtomValue(uncommittedChangesWithPreviews);
  const hasUncommittedChanges = uncommittedChanges.length > 0;

  const onClick = async () => {
    if (!(await confirmUnsavedEditsBeforeSplit([commit], 'split'))) {
      return;
    }
    setEditStackIntentionHashes(['split', new Set([commit.hash])]);
    if (trackerEventName === 'SplitOpenFromSplitSuggestion') {
      bumpStackEditMetric('splitFromSuggestion');
    }
    tracker.track(trackerEventName);
  };
  return (
    <Tooltip
      title={hasUncommittedChanges ? t('Cannot currently split with uncommitted changes') : ''}
      trigger={hasUncommittedChanges ? 'hover' : 'disabled'}>
      <Button onClick={onClick} disabled={hasUncommittedChanges} {...rest}>
        <SplitCommitIcon />
        <T>Split</T>
      </Button>
    </Tooltip>
  );
}
