/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// This file is loaded before the rest of the ISL webview.
// We define global platform data here that the rest of the app can use

import type {Platform} from 'isl/src/platform';

// important: this import should not transitively import code
// which depends on `window.islPlatform`, or else it won't be defined yet.
import {vscodeWebviewPlatform} from './vscodeWebviewPlatform';

window.islPlatform = vscodeWebviewPlatform;
__webpack_nonce__ = window.webpackNonce;

declare global {
  interface Window {
    islPlatform?: Platform;
    webpackNonce: string;
  }
  let __webpack_nonce__: string;
}
