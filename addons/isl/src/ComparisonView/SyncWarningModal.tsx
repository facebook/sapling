/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {SyncWarnings} from '../reviewComments';

import {Button} from 'isl-components/Button';
import {Modal} from '../Modal';
import {T} from '../i18n';

import './ComparisonView.css';

type Props = {
  warnings: SyncWarnings;
  onConfirm: () => void;
  onCancel: () => void;
};

export function SyncWarningModal({warnings, onConfirm, onCancel}: Props) {
  return (
    <Modal className="sync-warning-modal">
      <div className="sync-warning-content">
        <h3><T>Sync will affect your review progress</T></h3>

        <div className="sync-warning-details">
          {warnings.pendingCommentCount > 0 && (
            <p className="warning-item">
              <span className="warning-icon">Warning:</span>
              <T replace={{$count: warnings.pendingCommentCount}}>
                $count pending comment(s) may become invalid
              </T>
              <span className="warning-explanation">
                <T>(line numbers may shift after rebase, but comments are preserved)</T>
              </span>
            </p>
          )}

          {warnings.viewedFileCount > 0 && (
            <p className="warning-item">
              <span className="warning-icon">Info:</span>
              <T replace={{$count: warnings.viewedFileCount}}>
                $count viewed file(s) will be unmarked
              </T>
              <span className="warning-explanation">
                <T>(new commits require re-review)</T>
              </span>
            </p>
          )}
        </div>

        <div className="sync-warning-actions">
          <Button onClick={onCancel}>
            <T>Cancel</T>
          </Button>
          <Button primary onClick={onConfirm}>
            <T>Sync Anyway</T>
          </Button>
        </div>
      </div>
    </Modal>
  );
}
