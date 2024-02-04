/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import platform from './platform';
import {VSCodeLink} from '@vscode/webview-ui-toolkit/react';

export function Link({children, href}: {children: ReactNode; href: string}) {
  return <VSCodeLink onClick={() => platform.openExternalLink(href)}>{children}</VSCodeLink>;
}
