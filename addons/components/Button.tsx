/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ForwardedRef} from 'react';
import type {ExclusiveOr} from './Types';
import type {ReactProps} from './utils';

import * as stylex from '@stylexjs/stylex';
import {forwardRef, type ReactNode} from 'react';
import {layout} from './theme/layout';
import {colors} from './theme/tokens.stylex';

/**
 * StyleX tries to evaluate CSS variables and store them separately.
 * Use a layer of indirection so the CSS variable is used literally.
 */
export const vars = {
  fg: 'var(--foreground)',
  border: 'var(--contrast-border)',
  /** very bright border, usually only set in high-contrast themes */
  activeBorder: 'var(--contrast-active-border, transparent)',
  focusBorder: 'var(--focus-border, transparent)',
};

export const buttonStyles = stylex.create({
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
    flexWrap: 'nowrap',
    whiteSpace: 'nowrap',
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
      ':hover': 'var(--button-icon-hover-background, rgba(90, 93, 94, 0.31))',
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
      icon: iconProp,
      primary: primaryProp,
      disabled,
      onClick,
      children,
      xstyle,
      kind,
      className,
      ...rest
    }: {
      className?: string;
      children?: ReactNode;
      disabled?: boolean;
      xstyle?: stylex.StyleXStyles;
      primary?: boolean;
      icon?: boolean;
    } & Omit<ReactProps<HTMLButtonElement>, 'className'> &
      ExclusiveOr<
        ExclusiveOr<
          {
            /**
             * Render as a bright button, encouraged the primary confirmation action.
             * Equivalent to kind='primary'.
             */
            primary?: boolean;
          },
          {
            /**
             * Render as a smaller, more subtle button. Useful in toolbars or when using an icon instead of a label.
             * Equivalent to kind='icon'.
             */
            icon?: boolean;
          }
        >,
        /** How to display the button. Can also provide `primary` or `icon` shorthand bool props instead. */
        {kind?: 'primary' | 'icon' | undefined}
      >,
    ref: ForwardedRef<HTMLButtonElement>,
  ) => {
    const primary = kind === 'primary' || primaryProp === true;
    const icon = kind === 'icon' || iconProp === true;
    const {className: stylexClassName, ...otherStylex} = stylex.props(
      layout.flexRow,
      buttonStyles.button,
      primary && buttonStyles.primary,
      icon && buttonStyles.icon,
      disabled && buttonStyles.disabled,
      xstyle,
    );
    return (
      <button
        tabIndex={disabled ? -1 : 0}
        onClick={e => {
          // don't allow clicking a disabled button
          disabled !== true && onClick?.(e);
        }}
        ref={ref}
        className={stylexClassName + (className ? ' ' + className : '')}
        {...otherStylex}
        disabled={disabled}
        {...rest}>
        {children}
      </button>
    );
  },
);
