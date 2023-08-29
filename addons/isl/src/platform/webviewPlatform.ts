/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Platform} from '../platform';

import {browserPlatform} from '../BrowserPlatform';

/** Typed commands to communicate with the Tauri backend from the frontend */
type ExternalWebviewCommands = {
  openExternal: (url: string) => void;
};

declare const external: {
  invoke<K extends keyof ExternalWebviewCommands>(
    cmd: K,
    args: Parameters<ExternalWebviewCommands[K]>,
  ): Promise<ReturnType<ExternalWebviewCommands[K]>>;
};

// important: this file should not try to import other code from 'isl',
// since it will end up getting duplicated by webpack.

const webviewPlatform: Platform = {
  ...browserPlatform, // just act like the browser platform by default, since the app use case is similar
  platformName: 'webview',
  openExternalLink(url: string) {
    external.invoke('openExternal', [url]);
  },
};

window.islPlatform = webviewPlatform;
