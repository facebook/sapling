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

import {t} from './i18n';
import * as pathModule from 'path';
import * as vscode from 'vscode';

export const VSCodePlatform: ServerPlatform = {
  handleMessageFromClient: async (
    repo: Repository,
    message: PlatformSpecificClientToServerMessages,
    postMessage: (message: ServerToClientMessage) => void,
  ) => {
    try {
      switch (message.type) {
        case 'platform/openFile': {
          const path: AbsolutePath = pathModule.join(repo.info.repoRoot, message.path);
          const uri = vscode.Uri.file(path);
          vscode.window.showTextDocument(uri);
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
