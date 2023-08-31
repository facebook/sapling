/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from './types';

import {FileStackEditButton} from './FileStackEditButton';
import {UndoDescription} from './StackEditSubTree';
import {Tooltip, DOCUMENTATION_DELAY} from './Tooltip';
import {T, t} from './i18n';
import {ImportStackOperation} from './operations/ImportStackOperation';
import {hasExperimentalFeatures, latestHeadCommit, useRunOperation} from './serverAPIState';
import {
  bumpStackEditMetric,
  editingStackHashes,
  sendStackEditMetrics,
  useStackEditState,
} from './stackEditState';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useRecoilValue, useSetRecoilState} from 'recoil';
import {Icon} from 'shared/Icon';

export function StackEditConfirmButtons(): React.ReactElement {
  const setStackHashes = useSetRecoilState(editingStackHashes);
  const originalHead = useRecoilValue(latestHeadCommit);
  const runOperation = useRunOperation();
  const stackEdit = useStackEditState();
  const hasExperimental = useRecoilValue(hasExperimentalFeatures);

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

  const handleSaveChanges = () => {
    const importStack = stackEdit.commitStack.calculateImportStack({
      goto: originalHead?.hash,
      rewriteDate: Date.now() / 1000,
    });
    const op = new ImportStackOperation(importStack);
    runOperation(op);
    sendStackEditMetrics(true);
    // Exit stack editing.
    setStackHashes(new Set());
  };

  const handleCancel = () => {
    sendStackEditMetrics(false);
    setStackHashes(new Set<Hash>());
  };

  // Show [Edit file stack] [Cancel] [Save changes] [Undo] [Redo].
  return (
    <>
      <Tooltip
        title={t('Discard stack editing changes')}
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
        title={t('Save stack editing changes')}
        delayMs={DOCUMENTATION_DELAY}
        placement="bottom">
        <VSCodeButton
          className="confirm-edit-stack-button"
          appearance="primary"
          onClick={handleSaveChanges}>
          <T>Save changes</T>
        </VSCodeButton>
      </Tooltip>
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
      {hasExperimental && (
        <>
          <Icon icon="circle-small-filled" />
          <FileStackEditButton />
        </>
      )}
    </>
  );
}
