/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from '../../types';

import {Tooltip} from '../../Tooltip';
import {tracker} from '../../analytics';
import {T, t} from '../../i18n';
import {SplitCommitIcon} from '../../icons/SplitCommitIcon';
import {uncommittedChangesWithPreviews} from '../../previews';
import {useConfirmUnsavedEditsBeforeSplit} from './ConfirmUnsavedEditsBeforeSplit';
import {editingStackIntentionHashes} from './stackEditState';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useRecoilValue, useSetRecoilState} from 'recoil';

/** Button to open split UI for the current commit. Expected to be shown on the head commit.
 * Loads that one commit in the split UI. */
export function SplitButton({commit}: {commit: CommitInfo}) {
  const confirmUnsavedEditsBeforeSplit = useConfirmUnsavedEditsBeforeSplit();
  const setEditStackIntentionHashes = useSetRecoilState(editingStackIntentionHashes);

  const uncommittedChanges = useRecoilValue(uncommittedChangesWithPreviews);
  const hasUncommittedChanges = uncommittedChanges.length > 0;

  const onClick = async () => {
    if (!(await confirmUnsavedEditsBeforeSplit([commit], 'split'))) {
      return;
    }
    setEditStackIntentionHashes(['split', new Set([commit.hash])]);
    tracker.track('SplitOpenFromHeadCommit');
  };
  return (
    <Tooltip
      title={hasUncommittedChanges ? t('Cannot currently split with uncommitted changes') : ''}
      trigger={hasUncommittedChanges ? 'hover' : 'disabled'}>
      <VSCodeButton appearance="icon" onClick={onClick} disabled={hasUncommittedChanges}>
        <SplitCommitIcon slot="start" />
        <T>Split</T>
      </VSCodeButton>
    </Tooltip>
  );
}
