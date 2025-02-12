/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import * as stylex from '@stylexjs/stylex';
import {colors, font} from './theme/tokens.stylex';

const styles = stylex.create({
  linkButton: {
    fontSize: font.normal,
    textDecoration: 'underline',
    border: 'none',
    background: 'none',
    margin: 0,
    padding: 0,
    color: colors.fg,
    cursor: 'pointer',
    ':hover': {
      color: colors.brightFg,
    },
  },
});

export function LinkButton({
  children,
  onClick,
  style,
}: {
  children: ReactNode;
  onClick: () => unknown;
  style?: stylex.StyleXStyles;
}) {
  return (
    <button {...stylex.props(styles.linkButton, style)} onClick={onClick}>
      {children}
    </button>
  );
}
