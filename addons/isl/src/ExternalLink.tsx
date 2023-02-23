/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ComponentProps, ReactNode} from 'react';

import platform from './platform';
import {VSCodeLink} from '@vscode/webview-ui-toolkit/react';

/**
 * Link which opens url in a new browser tab
 */
export function ExternalLink(
  props: {href?: string; children: ReactNode; className?: string} & ComponentProps<
    typeof VSCodeLink
  >,
) {
  const {href, children, ...otherProps} = props;
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
    event.preventDefault();
    event.stopPropagation();
  };
  return (
    <VSCodeLink
      href={href}
      // Allow links to be focused
      tabIndex={0}
      onKeyUp={handleClick}
      onClick={handleClick}
      {...otherProps}>
      {children}
    </VSCodeLink>
  );
}
