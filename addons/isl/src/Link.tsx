/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ExclusiveOr} from 'shared/typeUtils';

import platform from './platform';
import {VSCodeLink} from '@vscode/webview-ui-toolkit/react';

export function Link(
  props: React.ComponentProps<typeof VSCodeLink> &
    ExclusiveOr<{href: string}, {onClick: () => unknown}>,
) {
  const {children, href, onClick, ...rest} = props;
  return (
    <VSCodeLink {...rest} onClick={href != null ? () => platform.openExternalLink(href) : onClick}>
      {children}
    </VSCodeLink>
  );
}
