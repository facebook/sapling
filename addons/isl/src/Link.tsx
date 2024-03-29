/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import platform from './platform';
import * as stylex from '@stylexjs/stylex';

const styles = stylex.create({
  a: {
    color: 'var(--link-foreground)',
    cursor: 'pointer',
    textDecoration: {
      ':hover': 'underline',
    },
    outline: {
      default: 'none',
      ':focus-visible': '1px solid var(--focus-border)',
    },
  },
});

export function Link({
  children,
  href,
  onClick,
  xstyle,
  ...rest
}: React.DetailedHTMLProps<React.AnchorHTMLAttributes<HTMLAnchorElement>, HTMLAnchorElement> & {
  xstyle?: stylex.StyleXStyles;
}) {
  return (
    <a
      tabIndex={0}
      {...stylex.props(styles.a, xstyle)}
      {...rest}
      onClick={href != null ? () => platform.openExternalLink(href) : onClick}>
      {children}
    </a>
  );
}
