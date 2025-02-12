/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ForwardedRef} from 'react';
import type {ReactProps} from './utils';

import * as stylex from '@stylexjs/stylex';
import {forwardRef} from 'react';
import {textFieldStyles} from './TextField';

const styles = stylex.create({
  horizontalGrowContainer: {
    display: 'inline-grid',
    maxWidth: '600px',
    alignItems: 'center',
    '::after': {
      width: 'auto',
      minWidth: '1em',
      content: 'attr(data-value)',
      visibility: 'hidden',
      whiteSpace: 'pre-wrap',
      gridArea: '1 / 2',
      height: '26px',
      padding: '0 9px',
    },
  },

  horizontalGrow: {
    width: 'auto',
    minWidth: '1em',
    gridArea: '1 / 2',
  },
});

/**
 * Like a normal text field / {@link TextField}, but grows horizontally to fit the text.
 */
export const HorizontallyGrowingTextField = forwardRef(
  (
    props: ReactProps<HTMLInputElement> & {
      value?: string;
      placeholder?: string;
    },
    ref: ForwardedRef<HTMLInputElement>,
  ) => {
    const {onInput, ...otherProps} = props;

    return (
      <div {...stylex.props(styles.horizontalGrowContainer)} data-value={otherProps.value}>
        <input
          {...stylex.props(textFieldStyles.input, styles.horizontalGrow)}
          type="text"
          ref={ref}
          onInput={e => {
            if ((e.currentTarget.parentNode as HTMLDivElement)?.dataset) {
              // Use `dataset` + `content: attr(data-value)` to size an ::after element,
              // which auto-expands the containing div to fit the text.
              (e.currentTarget.parentNode as HTMLDivElement).dataset.value = e.currentTarget.value;
            }
            onInput?.(e);
          }}
          {...otherProps}
        />
      </div>
    );
  },
);
