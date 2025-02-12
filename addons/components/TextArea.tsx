/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ForwardedRef, ReactNode} from 'react';

import * as stylex from '@stylexjs/stylex';
import {forwardRef, useId} from 'react';
import {Column} from './Flex';

const styles = stylex.create({
  root: {
    gap: '2px',
  },
  label: {
    marginBlock: '0px',
  },
  textarea: {
    fontFamily: 'var(--font-family)',
    boxSizing: 'border-box',
    padding: '8px',
    minWidth: '100px',
    minHeight: '42px',
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

export type TextAreaProps = {
  children?: ReactNode;
  xstyle?: stylex.StyleXStyles;
  containerXstyle?: stylex.StyleXStyles;
  resize?: 'none' | 'vertical' | 'horizontal' | 'both';
} & React.DetailedHTMLProps<React.TextareaHTMLAttributes<HTMLTextAreaElement>, HTMLTextAreaElement>;

export const TextArea = forwardRef(
  (
    {children, xstyle, containerXstyle, resize = 'none', ...rest}: TextAreaProps,
    ref: ForwardedRef<HTMLTextAreaElement>,
  ) => {
    const id = useId();
    return (
      <Column xstyle={[styles.root, containerXstyle ?? null]} alignStart>
        {children && (
          <label htmlFor={id} {...stylex.props(styles.label)}>
            {children}
          </label>
        )}
        <textarea
          ref={ref}
          style={{resize}}
          {...stylex.props(styles.textarea, xstyle)}
          id={id}
          {...rest}
        />
      </Column>
    );
  },
);
