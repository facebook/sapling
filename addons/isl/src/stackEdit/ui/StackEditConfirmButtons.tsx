/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../../types';

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {DOCUMENTATION_DELAY, Tooltip} from 'isl-components/Tooltip';
import {useAtom, useAtomValue} from 'jotai';
import {useCallback} from 'react';
import serverAPI from '../../ClientToServerAPI';
import {
  editedCommitMessages,
  getDefaultEditedCommitMessage,
} from '../../CommitInfoView/CommitInfoState';
import {Internal} from '../../Internal';
import {tracker} from '../../analytics';
import {useFeatureFlagSync} from '../../featureFlags';
import {T, t} from '../../i18n';
import {writeAtom} from '../../jotaiUtils';
import {ImportStackOperation} from '../../operations/ImportStackOperation';
import {RebaseOperation} from '../../operations/RebaseOperation';
import {useRunOperation} from '../../operationsState';
import {latestDag, latestHeadCommit, repositoryInfo} from '../../serverAPIState';
import {exactRevset, succeedableRevset} from '../../types';
import {UndoDescription} from './StackEditSubTree';
import {
  bumpStackEditMetric,
  editingStackIntentionHashes,
  findStartEndRevs,
  sendStackEditMetrics,
  useStackEditState,
} from './stackEditState';

import './StackEditSubTree.css';

export function StackEditConfirmButtons(): React.ReactElement {
  const [[stackIntention], setStackIntentionHashes] = useAtom(editingStackIntentionHashes);
  const originalHead = useAtomValue(latestHeadCommit);
  const dag = useAtomValue(latestDag);
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
  const invalidateUnsavedCommitMessages = useCallback((commits: Array<Hash>) => {
    for (const hash of commits) {
      writeAtom(editedCommitMessages(hash), getDefaultEditedCommitMessage());
    }
  }, []);

  const handleSaveChanges = () => {
    const originalHash = originalHead?.hash;
    const stack = stackEdit.commitStack.applyAbsorbEdits();
    const isAbsorb = stackEdit.intention === 'absorb';
    const importStack = stack.calculateImportStack({
      goto: originalHash,
      rewriteDate: Date.now() / 1000,
      // Do not write anything to the working copy, for absorb (esp. with partial selection)
      skipWdir: isAbsorb,
      // Also, preserve dirty files. So if an absorb edit is left "unabsorbed" in the "wdir()",
      // it will be preserved without being dropped.
      preserveDirtyFiles: isAbsorb,
    });
    const op = new ImportStackOperation(importStack, stack.originalStack);
    runOperation(op);
    sendStackEditMetrics(stackEdit, true);

    invalidateUnsavedCommitMessages(stack.originalStack.map(c => c.node));

    // For standalone split, follow-up with a rebase.
    // Note: the rebase might fail with conflicted pending changes.
    // rebase is technically incorrect if the user edits the changes.
    // We should move the rebase logic to debugimportstack and make
    // it handle pending changes just fine.
    const stackTop = stack.originalStack.at(-1)?.node;
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
    sendStackEditMetrics(stackEdit, false);
    setStackIntentionHashes(['general', new Set<Hash>()]);
  };

  // Get the commit hash for AI Split feature
  const [startRev] = findStartEndRevs(stackEdit);
  const {commitStack} = stackEdit;
  const repo = useAtomValue(repositoryInfo);
  const repoPath = repo?.repoRoot;
  const enableDevmateSplit = useFeatureFlagSync(Internal.featureFlags?.DevmateSplitCommit) ?? false;

  // Get the commit hash from the start of the split range
  const startCommit = startRev != null ? commitStack.get(startRev) : null;
  const splitCommitHash =
    startCommit?.originalNodes != null ? [...startCommit.originalNodes][0] : null;

  const handleAISplit = () => {
    if (splitCommitHash == null) {
      return;
    }
    const numFilesInCommit = startCommit?.files?.size ?? 0;

    // Bump the metric to track clicks for acceptance rate calculation
    bumpStackEditMetric('clickedAiSplit');

    tracker.track('DevmateSplitWithDevmateButtonClicked', {
      extras: {
        action: 'SplitCommit',
        source: 'splitUI',
        commitHash: splitCommitHash,
        numFilesInCommit,
        stackIntention,
      },
    });
    serverAPI.postMessage({
      type: 'platform/splitCommitWithAI',
      diffCommit: splitCommitHash,
      repoPath,
    });
  };

  let cancelTooltip = t('Discard stack editing changes');
  let confirmTooltip = t('Save stack editing changes');
  let confirmText = t('Save changes');
  switch (stackIntention) {
    case 'split':
      cancelTooltip = t('Cancel split');
      confirmTooltip = t('Apply split changes');
      confirmText = t('Split');
      break;
    case 'absorb':
      cancelTooltip = t('Cancel absorb');
      confirmTooltip = t('Apply absorb changes');
      confirmText = t('Absorb');
      break;
  }

  // Show [AI Split] [Undo] [Redo] [Cancel] [Save changes].
  return (
    <>
      {stackIntention === 'split' &&
        enableDevmateSplit &&
        splitCommitHash != null &&
        Internal.AISplitButton && <Internal.AISplitButton onClick={handleAISplit} />}
      {enableDevmateSplit && splitCommitHash != null && <div className="stack-edit-spacer" />}
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
        <Button icon disabled={!canUndo} onClick={handleUndo}>
          <Icon icon="discard" />
        </Button>
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
        <Button icon disabled={!canRedo} onClick={handleRedo}>
          <Icon icon="redo" />
        </Button>
      </Tooltip>
      <Tooltip title={cancelTooltip} delayMs={DOCUMENTATION_DELAY} placement="bottom">
        <Button className="cancel-edit-stack-button" onClick={handleCancel}>
          <T>Cancel</T>
        </Button>
      </Tooltip>
      <Tooltip title={confirmTooltip} delayMs={DOCUMENTATION_DELAY} placement="bottom">
        <Button
          className="confirm-edit-stack-button"
          data-testid="confirm-edit-stack-button"
          primary
          onClick={handleSaveChanges}>
          {confirmText}
        </Button>
      </Tooltip>
    </>
  );
}
