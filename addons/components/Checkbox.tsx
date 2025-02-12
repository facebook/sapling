/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type react from 'react';
import type {ReactProps} from './utils';

import * as stylex from '@stylexjs/stylex';
import {useEffect, useId, useRef} from 'react';
import {layout} from './theme/layout';
import {spacing} from './theme/tokens.stylex';

const cssVarFocusWithinBorder = '--checkbox-focus-within-color';
const styles = stylex.create({
  label: {
    [cssVarFocusWithinBorder]: {
      default: 'var(--checkbox-border)',
      ':focus-within': 'var(--focus-border)',
    },
    cursor: 'pointer',
    alignItems: 'center',
    position: 'relative',
    outline: 'none',
    userSelect: 'none',
  },
  input: {
    opacity: 0,
    outline: 'none',
    appearance: 'none',
    position: 'absolute',
  },
  disabled: {
    opacity: 0.5,
    cursor: 'not-allowed',
  },
  withChildren: {
    marginRight: spacing.pad,
  },
  checkmark: {
    background: 'var(--checkbox-background)',
    borderRadius: '3px',
    width: '16px',
    height: '16px',
    border: '1px solid var(--checkbox-border)',
    borderColor: `var(${cssVarFocusWithinBorder})`,
    display: 'inline-block',
    color: 'var(--checkbox-foreground)',
    transition: '60ms transform ease-in-out',
  },
});

function Checkmark({checked}: {checked: boolean}) {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      xmlns="http://www.w3.org/2000/svg"
      fill={checked ? 'currentColor' : 'transparent'}
      {...stylex.props(styles.checkmark)}>
      <path
        fillRule="evenodd"
        clipRule="evenodd"
        d="M14.431 3.323l-8.47 10-.79-.036-3.35-4.77.818-.574 2.978 4.24 8.051-9.506.764.646z"></path>
    </svg>
  );
}

function Indeterminate() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      xmlns="http://www.w3.org/2000/svg"
      fill={'currentColor'}
      {...stylex.props(styles.checkmark)}>
      <rect x="4" y="4" height="8" width="8" rx="2" />
    </svg>
  );
}

export function Checkbox({
  children,
  checked,
  onChange,
  disabled,
  indeterminate,
  xstyle,
  ...rest
}: {
  children?: react.ReactNode;
  checked: boolean;
  /** "indeterminate" state is neither true nor false, and renders as a box instead of a checkmark.
   * Usually represents partial selection of children. */
  indeterminate?: boolean;
  disabled?: boolean;
  onChange?: (checked: boolean) => unknown;
  xstyle?: stylex.StyleXStyles;
} & Omit<ReactProps<HTMLInputElement>, 'onChange'>) {
  const id = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  // Indeterminate cannot be set in HTML, use an effect to synchronize
  useEffect(() => {
    if (inputRef.current) {
      inputRef.current.indeterminate = indeterminate === true;
    }
  }, [indeterminate]);
  return (
    <label
      htmlFor={id}
      {...stylex.props(
        layout.flexRow,
        styles.label,
        children != null && styles.withChildren,
        disabled && styles.disabled,
        xstyle,
      )}>
      <input
        ref={inputRef}
        type="checkbox"
        id={id}
        checked={checked}
        onChange={e => !disabled && onChange?.(e.target.checked)}
        disabled={disabled}
        {...stylex.props(styles.input)}
        {...rest}
      />
      {indeterminate === true ? <Indeterminate /> : <Checkmark checked={checked} />}
      {children}
    </label>
  );
}
