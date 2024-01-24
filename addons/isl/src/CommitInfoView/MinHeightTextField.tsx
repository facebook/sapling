/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {assert} from '../utils';
import {VSCodeTextArea} from '@vscode/webview-ui-toolkit/react';
import {forwardRef, type ForwardedRef, useEffect, type MutableRefObject} from 'react';

/**
 * Wrap `VSCodeTextArea` to auto-resize to minimum height and optionally disallow newlines.
 * Like a `VSCodeTextField` that has text wrap inside.
 */
export const MinHeightTextField = forwardRef(
  (
    props: React.ComponentProps<typeof VSCodeTextArea> & {
      onInput: (event: {target: {value: string}}) => unknown;
      keepNewlines?: boolean;
    },
    ref: ForwardedRef<typeof VSCodeTextArea>,
  ) => {
    const {onInput, keepNewlines, ...rest} = props;

    // ref could also be a callback ref; don't bother supporting that right now.
    assert(typeof ref === 'object', 'MinHeightTextArea requires ref object');

    // whenever the value is changed, recompute & apply the minimum height
    useEffect(() => {
      const r = ref as MutableRefObject<typeof VSCodeTextArea>;
      const current = r?.current as unknown as HTMLInputElement;
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
      <VSCodeTextArea
        ref={ref}
        {...rest}
        className={`min-height-text-area${rest.className ? ' ' + rest.className : ''}`}
        onInput={e => {
          const newValue = (e.target as HTMLInputElement)?.value;
          const result = keepNewlines
            ? newValue
            : // remove newlines so this acts like a textField rather than a textArea
              newValue.replace(/(\r|\n)/g, '');
          onInput({
            target: {
              value: result,
            },
          });
        }}
      />
    );
  },
);
