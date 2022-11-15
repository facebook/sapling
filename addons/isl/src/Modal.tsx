/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import './Modal.css';

export function Modal({
  className,
  children,
  width,
  height,
  'aria-labelledby': ariaLabelledBy,
  'aria-describedby': ariaDescribedBy,
}: {
  className?: string;
  children: React.ReactNode;
  width?: string | number;
  height?: number | string;
  'aria-labelledby'?: string;
  'aria-describedby'?: string;
}) {
  return (
    <div
      className="modal"
      role="dialog"
      aria-modal={true}
      aria-labelledby={ariaLabelledBy}
      aria-describedby={ariaDescribedBy}>
      <div className={`modal-contents ${className ?? ''}`} style={{width, height}}>
        {children}
      </div>
    </div>
  );
}
