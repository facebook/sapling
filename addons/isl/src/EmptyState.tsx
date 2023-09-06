/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import './EmptyState.css';

export function EmptyState({children, small}: {children: React.ReactNode; small?: boolean}) {
  return (
    <div className={'empty-state' + (small ? ' empty-state-small' : '')}>
      <div>{children}</div>
    </div>
  );
}
