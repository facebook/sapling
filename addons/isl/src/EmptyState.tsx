/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import './EmptyState.css';

export function EmptyState({children}: {children: React.ReactNode}) {
  return (
    <div className="empty-state">
      <div>{children}</div>
    </div>
  );
}
