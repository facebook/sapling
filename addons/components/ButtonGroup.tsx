/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as stylex from '@stylexjs/stylex';
import React from 'react';
import {colors} from './theme/tokens.stylex';

const styles = stylex.create({
  group: {
    display: 'flex',
    flexWrap: 'nowrap',
    // StyleX Hack to target Button children of the ButtonGroup
    ':not(#__unused__) > button:not(:first-child):not(:last-child)': {
      borderRadius: 0,
      borderLeft: '1px solid var(--button-secondary-foreground)',
    },
    // button may either be a direct child of the group, or one level deeper (e.g. wrapped in tooltip)
    ':not(#__unused__) > *:not(:first-child):not(:last-child) > button': {
      borderRadius: 0,
      borderLeft: '1px solid var(--button-secondary-foreground)',
    },
    ':not(#__unused__) > *:first-child > button': {
      borderTopRightRadius: 0,
      borderBottomRightRadius: 0,
    },
    ':not(#__unused__) > button:first-child': {
      borderTopRightRadius: 0,
      borderBottomRightRadius: 0,
    },
    ':not(#__unused__) > *:last-child > button': {
      borderTopLeftRadius: 0,
      borderBottomLeftRadius: 0,
      borderLeft: '1px solid var(--button-secondary-foreground)',
    },
    ':not(#__unused__) > button:last-child': {
      borderTopLeftRadius: 0,
      borderBottomLeftRadius: 0,
      borderLeft: '1px solid var(--button-secondary-foreground)',
    },
  },
  icon: {
    ':not(#__unused__) > button:not(:first-child):not(:last-child)': {
      borderLeftColor: colors.hoverDarken,
    },
    ':not(#__unused__) > *:not(:first-child):not(:last-child) > button': {
      borderLeftColor: colors.hoverDarken,
    },
    ':not(#__unused__) > button:last-child': {
      borderLeftColor: colors.hoverDarken,
    },
    ':not(#__unused__) > *:last-child > button': {
      borderLeftColor: colors.hoverDarken,
    },
  },
});

export function ButtonGroup({
  children,
  icon,
  ...rest
}: {
  children: React.ReactNode;
  /** If true, the border between buttons will be colored to match <Button icon> style buttons */
  icon?: boolean;
  'data-testId'?: string;
}) {
  return (
    <div {...stylex.props(styles.group, icon && styles.icon)} {...rest}>
      {children}
    </div>
  );
}
