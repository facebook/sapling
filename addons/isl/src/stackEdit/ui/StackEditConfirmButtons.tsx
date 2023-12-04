/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../../types';

import {editedCommitMessages} from '../../CommitInfoView/CommitInfoState';
import {Tooltip, DOCUMENTATION_DELAY} from '../../Tooltip';
import {T, t} from '../../i18n';
import {ImportStackOperation} from '../../operations/ImportStackOperation';
import {RebaseOperation} from '../../operations/RebaseOperation';
import {latestDag, latestHeadCommit, useRunOperation} from '../../serverAPIState';
import {exactRevset, succeedableRevset} from '../../types';
import {UndoDescription} from './StackEditSubTree';
import {
  bumpStackEditMetric,
  editingStackIntentionHashes,
  sendStackEditMetrics,
  useStackEditState,
} from './stackEditState';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useRecoilCallback, useRecoilState, useRecoilValue} from 'recoil';
import {Icon} from 'shared/Icon';

export function StackEditConfirmButtons(): React.ReactElement {
  const [[stackIntention], setStackIntentionHashes] = useRecoilState(editingStackIntentionHashes);
  const originalHead = useRecoilValue(latestHeadCommit);
  const dag = useRecoilValue(latestDag);
  const runOperation = useRunOperation();
  const stackEdit = useStackEditState();

  const canUndo = stackEdit.canUndo();
  const canRedo = stackEdit.canRedo();

  const handleUndo = () => {
    stackEdit.undo();
    bumpStackEditMetric('undo');
  };

  const handleRedo = () => {
    stackEdit.redo();
    bumpStackEditMetric('redo');
  };

  /**
   * Invalidate any unsaved edited commit messages for the original commits,
   * to prevent detected successions from persisting that state.
   * Splitting can cause the top of the stack to be an unexpected
   * successor, leading to wrong commit messages.
   * We already showed a confirm modal to "apply" your edits to split,
   * but we actually need to delete them now that we're really
   * doing the split/edit stack.
   */
  const invalidateUnsavedCommitMessages = useRecoilCallback(
    ({reset}) =>
      (commits: Array<Hash>) => {
        for (const hash of commits) {
          reset(editedCommitMessages(hash));
        }
      },
    [],
  );

  const handleSaveChanges = () => {
    const originalHash = originalHead?.hash;
    const importStack = stackEdit.commitStack.calculateImportStack({
      goto: originalHash,
      rewriteDate: Date.now() / 1000,
    });
    const op = new ImportStackOperation(importStack, stackEdit.commitStack.originalStack);
    runOperation(op);
    sendStackEditMetrics(true);

    invalidateUnsavedCommitMessages(stackEdit.commitStack.originalStack.map(c => c.node));

    // For standalone split, follow-up with a rebase.
    // Note: the rebase might fail with conflicted pending changes.
    // rebase is technically incorrect if the user edits the changes.
    // We should move the rebase logic to debugimportstack and make
    // it handle pending changes just fine.
    const stackTop = stackEdit.commitStack.originalStack.at(-1)?.node;
    if (stackIntention === 'split' && stackTop != null) {
      const children = dag.children(stackTop);
      if (children.size > 0) {
        const rebaseOp = new RebaseOperation(
          exactRevset(children.toArray().join('|')),
          succeedableRevset(stackTop) /* stack top of the new successor */,
        );
        runOperation(rebaseOp);
      }
    }
    // Exit stack editing.
    setStackIntentionHashes(['general', new Set()]);
  };

  const handleCancel = () => {
    sendStackEditMetrics(false);
    setStackIntentionHashes(['general', new Set<Hash>()]);
  };

  // Show [Edit file stack] [Cancel] [Save changes] [Undo] [Redo].
  return (
    <>
      <Tooltip
        component={() =>
          canUndo ? (
            <T replace={{$op: <UndoDescription op={stackEdit.undoOperationDescription()} />}}>
              Undo $op
            </T>
          ) : (
            <T>No operations to undo</T>
          )
        }
        placement="bottom">
        <VSCodeButton appearance="icon" disabled={!canUndo} onClick={handleUndo}>
          <Icon icon="discard" />
        </VSCodeButton>
      </Tooltip>
      <Tooltip
        component={() =>
          canRedo ? (
            <T replace={{$op: <UndoDescription op={stackEdit.redoOperationDescription()} />}}>
              Redo $op
            </T>
          ) : (
            <T>No operations to redo</T>
          )
        }
        placement="bottom">
        <VSCodeButton appearance="icon" disabled={!canRedo} onClick={handleRedo}>
          <Icon icon="redo" />
        </VSCodeButton>
      </Tooltip>
      <Tooltip
        title={stackIntention === 'split' ? t('Cancel split') : t('Discard stack editing changes')}
        delayMs={DOCUMENTATION_DELAY}
        placement="bottom">
        <VSCodeButton
          className="cancel-edit-stack-button"
          appearance="secondary"
          onClick={handleCancel}>
          <T>Cancel</T>
        </VSCodeButton>
      </Tooltip>
      <Tooltip
        title={
          stackIntention === 'split' ? t('Apply split changes') : t('Save stack editing changes')
        }
        delayMs={DOCUMENTATION_DELAY}
        placement="bottom">
        <VSCodeButton
          className="confirm-edit-stack-button"
          appearance="primary"
          onClick={handleSaveChanges}>
          {stackIntention === 'split' ? <T>Split</T> : <T>Save changes</T>}
        </VSCodeButton>
      </Tooltip>
    </>
  );
}
