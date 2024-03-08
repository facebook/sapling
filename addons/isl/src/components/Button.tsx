/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactProps} from '../ComponentUtils';
import type {ForwardedRef} from 'react';
import type {ExclusiveOr} from 'shared/typeUtils';

import {layout} from '../stylexUtils';
import {colors} from '../tokens.stylex';
import * as stylex from '@stylexjs/stylex';
import {forwardRef, type ReactNode} from 'react';

const styles = stylex.create({
  button: {
    background: {
      default: 'var(--button-secondary-background)',
      ':hover': 'var(--button-secondary-hover-background)',
    },
    color: 'var(--button-secondary-foreground)',
    border: '1px solid var(--button-border)',
    borderRadius: '2px',
    padding: 'var(--button-padding-vertical) var(--button-padding-horizontal)',
    fontFamily: 'var(--font-family)',
    lineHeight: '16px',
    cursor: 'pointer',
    gap: '8px',
    outlineOffset: '2px',
    outlineStyle: 'solid',
    outlineWidth: '1px',
    outlineColor: {
      default: 'transparent',
      ':focus-visible': colors.focusBorder,
    },
  },
  primary: {
    background: {
      default: 'var(--button-primary-background)',
      ':hover': 'var(--button-primary-hover-background)',
    },
  },
  icon: {
    border: '1px solid',
    borderColor: colors.subtleHoverDarken,
    background: {
      default: colors.subtleHoverDarken,
      ':hover': 'var(--button-icon-hover-background)',
    },
    borderRadius: 'var(--button-icon-corner-radius)',
    color: colors.fg,
    padding: '3px',
    outlineOffset: 0,
  },
  disabled: {
    opacity: 'var(--disabled-opacity)',
    cursor: 'not-allowed',
  },
});

export const Button = forwardRef(
  (
    {
      icon,
      primary,
      disabled,
      onClick,
      children,
      xstyle,
      ...rest
    }: {
      children?: ReactNode;
      disabled?: boolean;
      xstyle?: stylex.StyleXStyles;
    } & ReactProps<HTMLButtonElement> &
      ExclusiveOr<{primary?: boolean}, {icon?: boolean}>,
    ref: ForwardedRef<HTMLButtonElement>,
  ) => {
    return (
      <button
        onClick={e => {
          // don't allow clicking a disabled button
          disabled !== true && onClick?.(e);
        }}
        ref={ref}
        {...stylex.props(
          layout.flexRow,
          styles.button,
          primary && styles.primary,
          icon && styles.icon,
          disabled && styles.disabled,
          xstyle,
        )}
        {...rest}>
        {children}
      </button>
    );
  },
);
