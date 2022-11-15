/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {AnchorHTMLAttributes, DetailedHTMLProps, ReactNode} from 'react';

import platform from './platform';

/**
 * Link which opens url in a new browser tab
 */
export function ExternalLink(
  props: {url?: string; children: ReactNode; className?: string} & DetailedHTMLProps<
    AnchorHTMLAttributes<HTMLAnchorElement>,
    HTMLAnchorElement
  >,
) {
  const {url, children, ...otherProps} = props;
  const handleClick = (
    event: React.MouseEvent<HTMLAnchorElement> | React.KeyboardEvent<HTMLAnchorElement>,
  ) => {
    // allow pressing Enter when focused to simulate clicking for accessability
    if (event.type === 'keyup') {
      if ((event as React.KeyboardEvent<HTMLAnchorElement>).key !== 'Enter') {
        return;
      }
    }
    if (url) {
      platform.openExternalLink(url);
    }
    event.preventDefault();
    event.stopPropagation();
  };
  return (
    <a
      href={url}
      target="_blank"
      // Allow links to be focused
      tabIndex={0}
      onKeyUp={handleClick}
      onClick={handleClick}
      {...otherProps}>
      {children}
    </a>
  );
}
