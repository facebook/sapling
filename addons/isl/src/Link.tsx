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
  const handleClick = (
    event: React.MouseEvent<HTMLAnchorElement> | React.KeyboardEvent<HTMLAnchorElement>,
  ) => {
    // allow pressing Enter when focused to simulate clicking for accessability
    if (event.type === 'keyup') {
      if ((event as React.KeyboardEvent<HTMLAnchorElement>).key !== 'Enter') {
        return;
      }
    }
    if (href) {
      platform.openExternalLink(href);
    }
    onClick?.(event as React.MouseEvent<HTMLAnchorElement>);
    event.preventDefault();
    event.stopPropagation();
  };
  return (
    <a
      href={href}
      tabIndex={0}
      onKeyUp={handleClick}
      onClick={handleClick}
      {...stylex.props(styles.a, xstyle)}
      {...rest}>
      {children}
    </a>
  );
}
