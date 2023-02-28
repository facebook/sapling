/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Repository} from 'isl-server/src/Repository';
import type {ServerPlatform} from 'isl-server/src/serverPlatform';
import type {
  AbsolutePath,
  PlatformSpecificClientToServerMessages,
  ServerToClientMessage,
} from 'isl/src/types';

import {executeVSCodeCommand} from './commands';
import {t} from './i18n';
import * as pathModule from 'path';
import * as vscode from 'vscode';

export const VSCodePlatform: ServerPlatform = {
  platformName: 'vscode',
  handleMessageFromClient: async (
    repo: Repository | undefined,
    message: PlatformSpecificClientToServerMessages,
    postMessage: (message: ServerToClientMessage) => void,
  ) => {
    try {
      switch (message.type) {
        case 'platform/openFile': {
          if (repo == null) {
            break;
          }
          const path: AbsolutePath = pathModule.join(repo.info.repoRoot, message.path);
          const uri = vscode.Uri.file(path);
          const editorPromise = vscode.window.showTextDocument(uri);
          const line = message.options?.line;
          if (line != null) {
            const editor = await editorPromise;
            const lineZeroIndexed = line - 1; // vscode uses 0-indexed line numbers
            editor.selections = [new vscode.Selection(lineZeroIndexed, 0, lineZeroIndexed, 0)]; // move cursor to line
            editor.revealRange(
              new vscode.Range(lineZeroIndexed, 0, lineZeroIndexed, 0),
              vscode.TextEditorRevealType.InCenterIfOutsideViewport,
            ); // scroll to line
          }
          break;
        }
        case 'platform/openDiff': {
          if (repo == null) {
            break;
          }
          const path: AbsolutePath = pathModule.join(repo.info.repoRoot, message.path);
          const uri = vscode.Uri.file(path);
          executeVSCodeCommand('sapling.open-file-diff', uri, message.comparison);
          break;
        }
        case 'platform/openExternal': {
          vscode.env.openExternal(vscode.Uri.parse(message.url));
          break;
        }
        case 'platform/confirm': {
          const OKButton = t('isl.confirmModalOK');
          const result = await vscode.window.showInformationMessage(
            message.message,
            {
              detail: message.details,
              modal: true,
            },
            OKButton,
          );
          postMessage({type: 'platform/confirmResult', result: result === OKButton});
          break;
        }
      }
    } catch (err) {
      vscode.window.showErrorMessage(`error handling message ${JSON.stringify(message)}\n${err}`);
    }
  },
};
