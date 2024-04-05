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

/**
 * StyleX tries to evaluate CSS variables and store them separately.
 * Use a layer of indirection so the CSS variable is used literally.
 */
export const vars = {
  fg: 'var(--foreground)',
  border: 'var(--contrast-border)',
  /** very bright border, usually only set in high-contrast themes */
  activeBorder: 'var(--contrast-active-border)',
  focusBorder: 'var(--focus-border)',
};

const styles = stylex.create({
  button: {
    background: {
      default: 'var(--button-secondary-background)',
      ':hover': 'var(--button-secondary-hover-background)',
    },
    color: 'var(--button-secondary-foreground)',
    border: '1px solid var(--button-border)',
    borderRadius: '2px',
    padding: '4px 11px',
    fontFamily: 'var(--font-family)',
    lineHeight: '16px',
    cursor: 'pointer',
    gap: '8px',
    outlineOffset: '2px',
    outlineStyle: 'solid',
    outlineWidth: '1px',
    outlineColor: {
      default: 'transparent',
      ':focus-visible': vars.focusBorder,
    },
  },
  primary: {
    background: {
      default: 'var(--button-primary-background)',
      ':hover': 'var(--button-primary-hover-background)',
    },
    color: 'var(--button-primary-foreground)',
  },
  icon: {
    border: '1px solid',
    borderColor: colors.subtleHoverDarken,
    background: {
      default: colors.subtleHoverDarken,
      ':hover': 'var(--button-icon-hover-background)',
    },
    borderRadius: '5px',
    color: vars.fg,
    padding: '3px',
    outlineStyle: {
      default: 'solid',
      ':hover': 'dotted',
      ':focus-within': 'solid',
    },
    outlineOffset: 0,
    outlineColor: {
      default: 'transparent',
      ':hover': vars.activeBorder,
      ':focus-visible': vars.focusBorder,
    },
  },
  disabled: {
    opacity: '0.4',
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
    } & Omit<ReactProps<HTMLButtonElement>, 'className'> &
      ExclusiveOr<{primary?: boolean}, {icon?: boolean}>,
    ref: ForwardedRef<HTMLButtonElement>,
  ) => {
    return (
      <button
        tabIndex={disabled ? -1 : 0}
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
        disabled={disabled}
        {...rest}>
        {children}
      </button>
    );
  },
);
