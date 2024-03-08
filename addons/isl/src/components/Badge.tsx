/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactProps} from '../ComponentUtils';
import type {ReactNode} from 'react';

import * as stylex from '@stylexjs/stylex';

const styles = stylex.create({
  badge: {
    display: 'inline-flex',
    alignItems: 'center',
    boxSizing: 'border-box',
    backgroundColor: 'var(--badge-background)',
    border: '1px solid var(--button-border)',
    borderRadius: '11px',
    color: 'var(--badge-foreground)',
    padding: '3px 6px',
    fontFamily: 'var(--font-family)',
    fontSize: '11px',
    minHeight: '18px',
    minWidth: '18px',
    lineHeight: '16px',
    height: '16px',

    textOverflow: 'ellipsis',
    maxWidth: '150px',
    whiteSpace: 'nowrap',
    overflow: 'hidden',
  },
});

export function Badge({
  xstyle,
  ...rest
}: {children: ReactNode; xstyle?: stylex.StyleXStyles} & ReactProps<HTMLSpanElement>) {
  return <span {...stylex.props(styles.badge, xstyle)} {...rest} />;
}
