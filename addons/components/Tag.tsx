/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';
import type {ReactProps} from './utils';

import * as stylex from '@stylexjs/stylex';

const styles = stylex.create({
  tag: {
    backgroundColor: 'var(--badge-background)',
    border: '1px solid var(--button-border)',
    borderRadius: 'var(--tag-corner-radius, 2px)',
    color: 'var(--badge-foreground)',
    padding: '2px 4px',
    fontFamily: 'var(--font-family)',
    fontSize: '11px',
    lineHeight: '16px',

    textOverflow: 'ellipsis',
    maxWidth: '150px',
    whiteSpace: 'nowrap',
    overflow: 'hidden',
  },
});

export function Tag({
  xstyle,
  ...rest
}: {children: ReactNode; xstyle?: stylex.StyleXStyles} & ReactProps<HTMLSpanElement>) {
  return <span {...stylex.props(styles.tag, xstyle)} {...rest} />;
}
