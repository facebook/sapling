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
  platformName: string;
  /** Override the analytics Session ID. Should be globally unique. */
  sessionId?: string;
  handleMessageFromClient(
    repo: Repository | undefined,
    message: PlatformSpecificClientToServerMessages,
    postMessage: (message: ServerToClientMessage) => void,
    onDispose: (disapose: () => unknown) => void,
  ): void | Promise<void>;
}

export const browserServerPlatform: ServerPlatform = {
  platformName: 'browser',
  handleMessageFromClient: (
    repo: Repository | undefined,
    message: PlatformSpecificClientToServerMessages,
  ) => {
    switch (message.type) {
      case 'platform/openContainingFolder': {
        const absPath: AbsolutePath = pathModule.join(unwrap(repo?.info.repoRoot), message.path);
        let args: Array<string> = [];
        // use OS-builtin open command to open parent directory
        // (which may open different file extensions with different programs)
        switch (process.platform) {
          case 'darwin':
            args = ['/usr/bin/open', pathModule.dirname(absPath)];
            break;
          case 'win32':
            // On windows, we can select the file in the newly opened explorer window by giving the full path
            args = ['explorer.exe', '/select,', absPath];
            break;
          case 'linux':
            args = ['xdg-open', pathModule.dirname(absPath)];
            break;
        }
        repo?.logger.log('open file', absPath);
        if (args.length > 0) {
          spawnInBackground(repo, args);
        }
        break;
      }
      case 'platform/openFile': {
        const absPath: AbsolutePath = pathModule.join(unwrap(repo?.info.repoRoot), message.path);
        let args: Array<string> = [];
        // use OS-builtin open command to open files
        // (which may open different file extensions with different programs)
        // TODO: add a config option to determine which program to launch
        switch (process.platform) {
          case 'darwin':
            args = ['/usr/bin/open', absPath];
            break;
          case 'win32':
            args = ['notepad.exe', absPath];
            break;
          case 'linux':
            args = ['xdg-open', absPath];
            break;
        }
        repo?.logger.log('open file', absPath);
        if (args.length > 0) {
          spawnInBackground(repo, args);
        }
        break;
      }
    }
  },
};

/**
 * Because the ISL server is likely running in the background and is
 * no longer attached to a terminal, this is designed for the case
 * where the user opens the file in a windowed editor (hence
 * `windowsHide: false`, which is the default for
 * `child_process.spawn()`, but not for `execa()`):
 *
 * - For users using a simple one-window-per-file graphical text
 *   editor, like notepad.exe, this is relatively straightforward.
 * - For users who prefer a terminal-based editor, like Emacs,
 *   a conduit like EmacsClient would be required.
 *
 * Further, killing ISL should not kill the editor, so this follows
 * the pattern for spawning an independent, long-running process in
 * Node.js as described here:
 *
 * https://nodejs.org/docs/latest-v10.x/api/child_process.html#child_process_options_detached
 */
function spawnInBackground(repo: Repository | undefined, args: Array<string>) {
  // TODO: Report error if spawn() fails?
  // TODO: support passing the column/line number to programs that support it? e.g. vscode: `code /path/to/file:10:20`
  const proc = spawn(args[0], args.slice(1), {
    detached: true,
    stdio: 'ignore',
    windowsHide: false,
    windowsVerbatimArguments: true,
  });
  // Silent error. Don't crash the server process.
  proc.on('error', err => {
    repo?.logger.log('failed to open', args, err);
  });
  proc.unref();
}
