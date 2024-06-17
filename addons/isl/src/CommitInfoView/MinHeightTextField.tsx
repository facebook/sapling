/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TextAreaProps} from '../components/TextArea';

import {TextArea} from '../components/TextArea';
import {assert} from '../utils';
import {forwardRef, type ForwardedRef, useEffect} from 'react';

/**
 * Wrap `VSCodeTextArea` to auto-resize to minimum height and optionally disallow newlines.
 * Like a `VSCodeTextField` that has text wrap inside.
 */
export const MinHeightTextField = forwardRef(
  (
    props: TextAreaProps & {
      onInput: (event: {currentTarget: HTMLTextAreaElement}) => unknown;
      keepNewlines?: boolean;
    },
    ref: ForwardedRef<HTMLTextAreaElement>,
  ) => {
    const {onInput, keepNewlines, ...rest} = props;

    // ref could also be a callback ref; don't bother supporting that right now.
    assert(typeof ref === 'object', 'MinHeightTextArea requires ref object');

    // whenever the value is changed, recompute & apply the minimum height
    useEffect(() => {
      const current = ref?.current;
      // height must be applied to textarea INSIDE shadowRoot of the VSCodeTextArea
      const innerTextArea = current?.shadowRoot?.querySelector('textarea');
      if (innerTextArea) {
        const resize = () => {
          innerTextArea.style.height = '';
          const scrollheight = innerTextArea.scrollHeight;
          innerTextArea.style.height = `${scrollheight}px`;
          innerTextArea.rows = 1;
        };
        resize();
        const obs = new ResizeObserver(resize);
        obs.observe(innerTextArea);
        return () => obs.unobserve(innerTextArea);
      }
    }, [props.value, ref]);

    return (
      <TextArea
        ref={ref}
        {...rest}
        className={`min-height-text-area${rest.className ? ' ' + rest.className : ''}`}
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
