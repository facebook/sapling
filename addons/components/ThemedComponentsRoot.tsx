/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';
import type {Writable} from 'shared/typeUtils';

import {light} from './theme/tokens.stylex';
import * as stylex from '@stylexjs/stylex';

export function ThemedComponentsRoot({
  theme,
  className,
  children,
}: {
  theme: 'light' | 'dark';
  className?: string;
  children: ReactNode;
}) {
  const props = stylex.props(theme === 'light' && light);
  // stylex would overwrite className
  (props as Writable<typeof props>).className += ` ${className ?? ''} ${theme}-theme`;
  return <div {...props}>{children}</div>;
}
