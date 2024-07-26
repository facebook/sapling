/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import {DropdownField} from './DropdownFields';

export function Setting({
  children,
  title,
  description,
}: {
  children: ReactNode;
  title: ReactNode;
  description?: ReactNode;
}) {
  return (
    <DropdownField title={title}>
      {description && <div className="setting-description">{description}</div>}
      {children}
    </DropdownField>
  );
}
