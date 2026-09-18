/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import LazyLoginDialog from './LazyLoginDialog';
import {setCustomLoginDialogComponent} from 'reviewstack/src/LoginDialog';
import {setCustomLogout} from 'reviewstack/src/Username';

export {
  App,
  getColorModeFromLocalStorage,
  JotaiProvider,
  setCustomLinkElement,
  setCustomNavigateHook,
  ThemeProvider,
} from 'reviewstack/src/index';

export function configureLoginDialog() {
  setCustomLoginDialogComponent(LazyLoginDialog);
  setCustomLogout(() => {
    window.location.assign('/github/oauth2/sign_out?rd=/');
  });
}
