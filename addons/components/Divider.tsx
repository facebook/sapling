/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactProps} from '../ComponentUtils';

import * as stylex from '@stylexjs/stylex';

const styles = stylex.create({
  hr: {
    margin: '4px 0',
    border: 'none',
    borderTop: '1px solid var(--divider-background)',
    outline: 'none',
    height: 0,
    width: '100%',
  },
});

export function Divider({
  xstyle,
}: {
  xstyle?: stylex.StyleXStyles;
} & ReactProps<HTMLHRElement>) {
  return <hr {...stylex.props(styles.hr, xstyle)} />;
}
