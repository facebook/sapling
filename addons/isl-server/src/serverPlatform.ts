/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Repository} from './Repository';
import type {
  AbsolutePath,
  PlatformSpecificClientToServerMessages,
  ServerToClientMessage,
} from 'isl/src/types';

import {spawn} from 'child_process';
import pathModule from 'path';
import {unwrap} from 'shared/utils';

/**
 * Platform-specific server-side API for each target: vscode extension host, electron standalone, browser, ...
 * See also platform.ts
 */
export interface ServerPlatform {
  handleMessageFromClient(
    repo: Repository | undefined,
    message: PlatformSpecificClientToServerMessages,
    postMessage: (message: ServerToClientMessage) => void,
  ): void | Promise<void>;
}

export const browserServerPlatform: ServerPlatform = {
  handleMessageFromClient: (
    repo: Repository | undefined,
    message: PlatformSpecificClientToServerMessages,
  ) => {
    switch (message.type) {
      case 'platform/openFile': {
        const path: AbsolutePath = pathModule.join(unwrap(repo?.info.repoRoot), message.path);
        let command;
        if (command == null) {
          // use OS-builtin open command to open files
          // (which may open different file extensions with different programs)
          // TODO: add a config option to determine which program to launch
          switch (process.platform) {
            case 'darwin':
              command = '/usr/bin/open';
              break;
            case 'win32':
              command = 'notepad.exe';
              break;
            case 'linux':
              command = 'xdg-open';
              break;
          }
        }
        if (command) {
          // Because the ISL server is likely running in the background and is
          // no longer attached to a terminal, this is designed for the case
          // where the user opens the file in a windowed editor (hence
          // `windowsHide: false`, which is the default for
          // `child_process.spawn()`, but not for `execa()`):
          //
          // - For users using a simple one-window-per-file graphical text
          //   editor, like notepad.exe, this is relatively straightforward.
          // - For users who prefer a terminal-based editor, like Emacs,
          //   a conduit like EmacsClient would be required.
          //
          // Further, killing ISL should not kill the editor, so this follows
          // the pattern for spawning an independent, long-running process in
          // Node.js as described here:
          //
          // https://nodejs.org/docs/latest-v10.x/api/child_process.html#child_process_options_detached
          repo?.logger.log('open file', path);
          // TODO: Report error if spawn() fails?
          const proc = spawn(command, [path], {
            detached: true,
            stdio: 'ignore',
            windowsHide: false,
            windowsVerbatimArguments: true,
          });
          proc.unref();
        }
        break;
      }
    }
  },
};
