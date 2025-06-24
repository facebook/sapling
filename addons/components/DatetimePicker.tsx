/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ForwardedRef, ReactNode} from 'react';
import type {ReactProps} from './utils';

import * as stylex from '@stylexjs/stylex';
import {forwardRef, useId} from 'react';
import {Column} from './Flex';

export const datetimePickerStyles = stylex.create({
  root: {
    gap: 0,
  },
  label: {
    marginBlock: '1px',
  },
  input: {
    boxSizing: 'border-box',
    height: '26px',
    padding: '0 9px',
    marginBlock: 0,
    minWidth: '100px',
    width: '100%',
    background: 'var(--input-background)',
    color: 'var(--input-foreground)',
    border: '1px solid var(--dropdown-border)',
    outline: {
      default: 'none',
      ':focus-visible': '1px solid var(--focus-border)',
    },
    outlineOffset: '-1px',
  },
});

export const DatetimePicker = forwardRef(
  (
    {
      children,
      xstyle,
      containerXstyle,
      value,
      max,
      width,
      ...rest
    }: {
      children?: ReactNode;
      xstyle?: stylex.StyleXStyles;
      containerXstyle?: stylex.StyleXStyles;
      value?: string;
      max?: string;
      width?: string;
      placeholder?: string;
      readOnly?: boolean;
    } & ReactProps<HTMLInputElement>,
    ref: ForwardedRef<HTMLInputElement>,
  ) => {
    const id = useId();
    return (
      <Column
        xstyle={[datetimePickerStyles.root, containerXstyle ?? null]}
        style={{width}}
        alignStart>
        {children && (
          <label htmlFor={id} {...stylex.props(datetimePickerStyles.label)}>
            {children}
          </label>
        )}
        <input
          {...stylex.props(datetimePickerStyles.input, xstyle)}
          type="datetime-local"
          id={id}
          value={value}
          max={max}
          {...rest}
          ref={ref}
        />
      </Column>
    );
  },
);
