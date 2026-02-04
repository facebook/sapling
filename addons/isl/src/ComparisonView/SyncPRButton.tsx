/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {DOCUMENTATION_DELAY, Tooltip} from 'isl-components/Tooltip';
import {useAtomValue} from 'jotai';
import {useState, useCallback} from 'react';
import {T, t} from '../i18n';
import {SyncPROperation} from '../operations/SyncPROperation';
import {useRunOperation, isOperationRunningAtom} from '../operationsState';
import {getSyncWarnings} from '../reviewComments';
import {SyncWarningModal} from './SyncWarningModal';

type Props = {
  prNumber: string;
  headHash: string;
};

export function SyncPRButton({prNumber, headHash}: Props) {
  const runOperation = useRunOperation();
  const isOperationRunning = useAtomValue(isOperationRunningAtom);
  const [showWarningModal, setShowWarningModal] = useState(false);
  const [pendingWarnings, setPendingWarnings] = useState<ReturnType<typeof getSyncWarnings> | null>(null);

  const handleClick = useCallback(() => {
    // Check for warnings before syncing
    const warnings = getSyncWarnings(prNumber, headHash);

    if (warnings.hasWarnings) {
      // Show confirmation modal
      setPendingWarnings(warnings);
      setShowWarningModal(true);
    } else {
      // No warnings, sync immediately
      runOperation(new SyncPROperation(prNumber));
    }
  }, [prNumber, headHash, runOperation]);

  const handleConfirmSync = useCallback(() => {
    setShowWarningModal(false);
    setPendingWarnings(null);
    runOperation(new SyncPROperation(prNumber));
  }, [prNumber, runOperation]);

  const handleCancelSync = useCallback(() => {
    setShowWarningModal(false);
    setPendingWarnings(null);
  }, []);

  return (
    <>
      <Tooltip
        placement="bottom"
        delayMs={DOCUMENTATION_DELAY}
        title={t('Sync PR branch with latest main (rebase)')}>
        <Button
          disabled={isOperationRunning}
          onClick={handleClick}>
          <Icon slot="start" icon={isOperationRunning ? 'loading' : 'sync'} />
          <T>Sync</T>
        </Button>
      </Tooltip>

      {showWarningModal && pendingWarnings && (
        <SyncWarningModal
          warnings={pendingWarnings}
          onConfirm={handleConfirmSync}
          onCancel={handleCancelSync}
        />
      )}
    </>
  );
}
