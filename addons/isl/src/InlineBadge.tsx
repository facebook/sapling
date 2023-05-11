/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import './InlineBadge.css';

export function InlineBadge({
  children,
  kind,
}: {
  children: ReactNode;
  kind?: 'primary' | 'secondary';
}) {
  return <div className={`inline-badge badge-${kind ?? 'secondary'}`}>{children}</div>;
}
