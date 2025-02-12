/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as stylex from '@stylexjs/stylex';
import {stylexPropsWithClassName} from './utils';

const styles = stylex.create({
  subtle: {
    fontSize: '90%',
    opacity: 0.9,
  },
});

export function Subtle({
  children,
  className,
  ...props
}: React.DetailedHTMLProps<React.HTMLAttributes<HTMLSpanElement>, HTMLSpanElement>) {
  return (
    <span {...stylexPropsWithClassName(styles.subtle, className)} {...props}>
      {children}
    </span>
  );
}
