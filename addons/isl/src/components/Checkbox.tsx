/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactProps} from '../ComponentUtils';
import type react from 'react';

import {layout} from '../stylexUtils';
import {spacing} from '../tokens.stylex';
import * as stylex from '@stylexjs/stylex';
import {useId} from 'react';

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
    marginRight: spacing.pad,
    userSelect: 'none',
  },
  input: {
    opacity: 0,
    outline: 'none',
    appearance: 'none',
    position: 'absolute',
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

export function Checkbox({
  children,
  checked,
  onChange,
  xstyle,
  ...rest
}: {
  children: react.ReactNode;
  checked: boolean;
  onChange: (checked: boolean) => unknown;
  xstyle?: stylex.StyleXStyles;
} & Omit<ReactProps<HTMLInputElement>, 'onChange'>) {
  const id = useId();
  return (
    <label htmlFor={id} {...stylex.props(layout.flexRow, styles.label, xstyle)}>
      <input
        type="checkbox"
        id={id}
        checked={checked}
        onChange={e => onChange(e.target.checked)}
        {...stylex.props(styles.input)}
        {...rest}
      />
      <Checkmark checked={checked} />
      {children}
    </label>
  );
}
