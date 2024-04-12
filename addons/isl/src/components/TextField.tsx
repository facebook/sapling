/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactProps} from '../ComponentUtils';
import type {ForwardedRef, ReactNode} from 'react';

import {Column} from '../ComponentUtils';
import * as stylex from '@stylexjs/stylex';
import {forwardRef, useId} from 'react';

const styles = stylex.create({
  root: {
    alignItems: 'flex-start',
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

export const TextField = forwardRef(
  (
    {
      children,
      xstyle,
      value,
      width,
      ...rest
    }: {
      children?: ReactNode;
      xstyle?: stylex.StyleXStyles;
      value?: string;
      width?: string;
    } & ReactProps<HTMLInputElement>,
    ref: ForwardedRef<HTMLInputElement>,
  ) => {
    const id = useId();
    return (
      <Column xstyle={styles.root} style={{width}}>
        {children && (
          <label htmlFor={id} {...stylex.props(styles.label)}>
            {children}
          </label>
        )}
        <input
          {...stylex.props(styles.input, xstyle)}
          type="text"
          id={id}
          value={value}
          {...rest}
          ref={ref}
        />
      </Column>
    );
  },
);
