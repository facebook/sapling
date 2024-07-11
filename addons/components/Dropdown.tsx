/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactProps} from '../ComponentUtils';
import type {ReactNode} from 'react';

import * as stylex from '@stylexjs/stylex';
import {useId} from 'react';

const styles = stylex.create({
  label: {
    marginBlock: '0px',
  },
  select: {
    fontFamily: 'var(--font-family)',
    boxSizing: 'border-box',
    padding: '3px 6px',
    background: 'var(--input-background)',
    color: 'var(--input-foreground)',
    border: '1px solid var(--dropdown-border)',
    outline: {
      default: 'none',
      ':focus-visible': '1px solid var(--focus-border)',
    },
    outlineOffset: '-1px',
  },
  disabled: {
    opacity: 0.4,
    cursor: 'not-allowed',
  },
});

export function Dropdown<T extends string | {value: string; name: string; disabled?: boolean}>({
  options,
  children,
  xstyle,
  value,
  disabled,
  ...rest
}: {
  options: Array<T>;
  children?: ReactNode;
  value?: T extends string ? T : T extends {value: string; name: string} ? T['value'] : never;
  disabled?: boolean;
  xstyle?: stylex.StyleXStyles;
} & ReactProps<HTMLSelectElement>) {
  const id = useId();
  return (
    <select
      {...stylex.props(styles.select, xstyle, disabled && styles.disabled)}
      {...rest}
      disabled={disabled || options.length === 0}
      value={value}>
      {children && (
        <label htmlFor={id} {...stylex.props(styles.label)}>
          {children}
        </label>
      )}
      {options.map((option, index) => {
        const val = typeof option === 'string' ? option : option.value;
        const name = typeof option === 'string' ? option : option.name;
        const disabled = typeof option === 'string' ? false : option.disabled;
        return (
          <option key={index} value={val} disabled={disabled}>
            {name}
          </option>
        );
      })}
    </select>
  );
}
