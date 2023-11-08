/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Platform} from '../platform';

import {browserPlatformImpl} from './browerPlatformImpl';

/** Typed commands to communicate with the Tauri backend from the frontend */
type TauriCommands = {
  // exampleCommand: [{exampleArg: string}, string];
};
// const {invoke} = window.__TAURI__.tauri;

declare global {
  interface Window {
    __TAURI__: {
      tauri: {
        invoke<K extends keyof TauriCommands>(
          cmd: K,
          args: TauriCommands[K][0],
        ): Promise<TauriCommands[K][1]>;
      };
    };
  }
}

// important: this file should not try to import other code from 'isl',
// since it will end up getting duplicated by webpack.

const standalonePlatform: Platform = {
  ...browserPlatformImpl, // just act like the browser platform by default, since the app use case is similar
  platformName: 'standalone',
};

window.islPlatform = standalonePlatform;
