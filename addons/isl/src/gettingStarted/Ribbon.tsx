/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import './Ribbon.css';

export function Ribbon({children}: {children: React.ReactNode}) {
  return (
    <div className="ribbon ribbon-top-right">
      <span>{children}</span>
    </div>
  );
}
