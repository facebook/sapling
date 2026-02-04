/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Icon} from 'isl-components/Icon';
import {useAtomValue} from 'jotai';
import {T} from '../i18n';
import {operationList, isOperationRunningAtom} from '../operationsState';
import {SyncPROperation} from '../operations/SyncPROperation';

import './ComparisonView.css';

type Props = {
  prNumber: string;
};

export function SyncProgress({prNumber}: Props) {
  const opList = useAtomValue(operationList);
  const isRunning = useAtomValue(isOperationRunningAtom);
  const currentOp = opList.currentOperation;

  // Only show progress for sync operations on this PR
  const isSyncOperation = currentOp?.operation instanceof SyncPROperation;
  // SyncPROperation.prNumber is public (defined in 13-01)
  const isThisPR = isSyncOperation &&
    (currentOp?.operation as SyncPROperation).prNumber === prNumber;

  if (!isThisPR) {
    return null;
  }

  // Operation still running
  if (isRunning) {
    const progress = currentOp?.currentProgress;
    return (
      <div className="sync-progress sync-progress-running">
        <Icon icon="loading" />
        <span className="sync-progress-text">
          {progress?.message || <T>Syncing PR with main...</T>}
        </span>
      </div>
    );
  }

  // Operation completed
  if (currentOp?.exitCode !== undefined) {
    const success = currentOp.exitCode === 0;
    return (
      <div className={`sync-progress ${success ? 'sync-progress-success' : 'sync-progress-error'}`}>
        <Icon icon={success ? 'check' : 'warning'} />
        <span className="sync-progress-text">
          {success ? (
            <T>PR synced with main</T>
          ) : (
            <T>Sync failed - check for conflicts</T>
          )}
        </span>
      </div>
    );
  }

  return null;
}
