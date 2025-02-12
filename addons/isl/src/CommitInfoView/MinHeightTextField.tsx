/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TextAreaProps} from 'isl-components/TextArea';

import * as stylex from '@stylexjs/stylex';
import {TextArea} from 'isl-components/TextArea';
import {forwardRef, useEffect, type ForwardedRef} from 'react';
import {notEmpty} from 'shared/utils';
import {assert} from '../utils';

const styles = stylex.create({
  minHeight: {
    overflow: 'hidden',
    minHeight: '26px',
  },
});

/**
 * Wrap `TextArea` to auto-resize to minimum height and optionally disallow newlines.
 * Like a `TextField` that has text wrap inside.
 */
export const MinHeightTextField = forwardRef(
  (
    props: TextAreaProps & {
      onInput: (event: {currentTarget: HTMLTextAreaElement}) => unknown;
      keepNewlines?: boolean;
      xstyle?: stylex.StyleXStyles;
      containerXstyle?: stylex.StyleXStyles;
    },
    ref: ForwardedRef<HTMLTextAreaElement>,
  ) => {
    const {onInput, keepNewlines, xstyle, ...rest} = props;

    // ref could also be a callback ref; don't bother supporting that right now.
    assert(typeof ref === 'object', 'MinHeightTextArea requires ref object');

    // whenever the value is changed, recompute & apply the minimum height
    useEffect(() => {
      const textarea = ref?.current;
      if (textarea) {
        const resize = () => {
          textarea.style.height = '';
          const scrollheight = textarea.scrollHeight;
          textarea.style.height = `${scrollheight}px`;
          textarea.rows = 1;
        };
        resize();
        const obs = new ResizeObserver(resize);
        obs.observe(textarea);
        return () => obs.unobserve(textarea);
      }
    }, [props.value, ref]);

    return (
      <TextArea
        ref={ref}
        {...rest}
        xstyle={[styles.minHeight, xstyle].filter(notEmpty)}
        onInput={e => {
          const newValue = e.currentTarget?.value;
          const result = keepNewlines
            ? newValue
            : // remove newlines so this acts like a textField rather than a textArea
              newValue.replace(/(\r|\n)/g, '');
          onInput({
            currentTarget: {
              value: result,
            } as HTMLTextAreaElement,
          });
        }}
      />
    );
  },
);
