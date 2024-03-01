/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import {Tag} from './components/Tag';
import * as stylex from '@stylexjs/stylex';

const styles = stylex.create({
  special: {
    backgroundColor: 'var(--list-hover-background)',
    color: 'var(--list-hover-foreground)',
  },
});

export function Bookmark({children, special}: {children: ReactNode; special?: boolean}) {
  return (
    <Tag
      xstyle={special !== true ? undefined : styles.special}
      title={typeof children === 'string' ? children : undefined}>
      {children}
    </Tag>
  );
}
