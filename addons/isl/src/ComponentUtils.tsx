/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Icon} from 'shared/Icon';

import './ComponentUtils.css';

export function LargeSpinner() {
  return (
    <div data-testid="loading-spinner">
      <Icon icon="loading" size="L" />
    </div>
  );
}

export function Center({children}: {children: React.ReactNode}) {
  return <div className="center-container">{children}</div>;
}
