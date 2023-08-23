/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {Icon} from 'shared/Icon';
import './Modal.css';

export function Modal({
  className,
  children,
  width,
  height,
  maxWidth,
  maxHeight,
  'aria-labelledby': ariaLabelledBy,
  'aria-describedby': ariaDescribedBy,
  dismiss,
}: {
  className?: string;
  children: React.ReactNode;
  width?: string | number;
  height?: number | string;
  maxWidth?: string | number;
  maxHeight?: string | number;
  'aria-labelledby'?: string;
  'aria-describedby'?: string;
  /** Callback to dismiss the modal. If provided, an 'x' button is added to the top-right corner of the modal. */
  dismiss?: () => void;
}) {
  return (
    <div
      className="modal"
      role="dialog"
      aria-modal={true}
      aria-labelledby={ariaLabelledBy}
      aria-describedby={ariaDescribedBy}>
      <div
        className={`modal-contents ${className ?? ''}`}
        style={{width, height, maxWidth, maxHeight}}>
        {dismiss != null ? (
          <div className="dismiss-modal">
            <VSCodeButton appearance="icon" onClick={dismiss}>
              <Icon icon="x" />
            </VSCodeButton>
          </div>
        ) : null}
        {children}
      </div>
    </div>
  );
}
