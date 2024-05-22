/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ServerPlatform} from 'isl-server/src/serverPlatform';
import type {RepositoryContext} from 'isl-server/src/serverTypes';
import type {
  AbsolutePath,
  PlatformSpecificClientToServerMessages,
  RepoRelativePath,
  ServerToClientMessage,
} from 'isl/src/types';
import type {Json} from 'shared/typeUtils';

import {executeVSCodeCommand} from './commands';
import {t} from './i18n';
import {Repository} from 'isl-server/src/Repository';
import {arraysEqual} from 'isl/src/utils';
import * as pathModule from 'node:path';
import * as vscode from 'vscode';

const IMAGE_EXTENSIONS = new Set(['.bmp', '.gif', '.ico', '.jpeg', '.jpg', '.png', '.webp']);
function looksLikeImageUri(uri: vscode.Uri): boolean {
  const ext = pathModule.extname(uri.path).toLowerCase();
  return IMAGE_EXTENSIONS.has(ext);
}

export const getVSCodePlatform = (context: vscode.ExtensionContext): ServerPlatform => ({
  platformName: 'vscode',
  sessionId: vscode.env.sessionId,
  handleMessageFromClient: async (
    repo: Repository | undefined,
    ctx: RepositoryContext,
    message: PlatformSpecificClientToServerMessages,
    postMessage: (message: ServerToClientMessage) => void,
    onDispose: (cb: () => unknown) => void,
  ) => {
    try {
      switch (message.type) {
        case 'platform/openFile': {
          if (repo == null) {
            break;
          }
          const path: AbsolutePath = pathModule.join(repo.info.repoRoot, message.path);
          const uri = vscode.Uri.file(path);
          if (looksLikeImageUri(uri)) {
            vscode.commands.executeCommand('vscode.open', uri).then(undefined, err => {
              vscode.window.showErrorMessage('cannot open file' + (err.message ?? String(err)));
            });
            return;
          }
          vscode.window.showTextDocument(uri).then(
            editor => {
              const line = message.options?.line;
              if (line != null) {
                const lineZeroIndexed = line - 1; // vscode uses 0-indexed line numbers
                editor.selections = [new vscode.Selection(lineZeroIndexed, 0, lineZeroIndexed, 0)]; // move cursor to line
                editor.revealRange(
                  new vscode.Range(lineZeroIndexed, 0, lineZeroIndexed, 0),
                  vscode.TextEditorRevealType.InCenterIfOutsideViewport,
                ); // scroll to line
              }
            },
            err => {
              vscode.window.showErrorMessage(err.message ?? String(err));
            },
          );
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
        case 'platform/subscribeToUnsavedFiles': {
          let previous: Array<{path: RepoRelativePath; uri: string}> = [];
          const postUnsavedFiles = () => {
            if (repo == null) {
              return;
            }
            const files = vscode.workspace.textDocuments
              .filter(
                document =>
                  (document.isDirty && repo.isPathInsideRepo(document.fileName)) ||
                  document.isUntitled,
              )
              .filter(document => document.isDirty || document.isUntitled)
              .map(document => {
                return {
                  path: pathModule.relative(repo.info.repoRoot, document.fileName),
                  uri: document.uri.toString(),
                };
              });

            if (!arraysEqual(files, previous)) {
              postMessage({
                type: 'platform/unsavedFiles',
                unsaved: files,
              });
              previous = files;
            }
          };

          const disposables = [
            vscode.workspace.onDidChangeTextDocument(postUnsavedFiles),
            vscode.workspace.onDidSaveTextDocument(postUnsavedFiles),
          ];
          postUnsavedFiles();
          onDispose(() => disposables.forEach(d => d.dispose()));
          break;
        }
        case 'platform/subscribeToAvailableCwds': {
          const postAllAvailableCwds = async () => {
            const options = await Promise.all(
              (vscode.workspace.workspaceFolders ?? []).map(folder => {
                const cwd = folder.uri.fsPath;
                return Repository.getCwdInfo({...ctx, cwd});
              }),
            );
            postMessage({
              type: 'platform/availableCwds',
              options,
            });
          };

          postAllAvailableCwds();
          const dispose = vscode.workspace.onDidChangeWorkspaceFolders(postAllAvailableCwds);
          onDispose(() => dispose.dispose());
          break;
        }
        case 'platform/setVSCodeConfig': {
          vscode.workspace
            .getConfiguration()
            .update(
              message.config,
              message.value,
              message.scope === 'global'
                ? vscode.ConfigurationTarget.Global
                : vscode.ConfigurationTarget.Workspace,
            );
          break;
        }
        case 'platform/setPersistedState': {
          const {data} = message;
          context.globalState.update('isl-persisted', data);
          break;
        }
        case 'platform/subscribeToVSCodeConfig': {
          const sendLatestValue = () =>
            postMessage({
              type: 'platform/vscodeConfigChanged',
              config: message.config,
              value: vscode.workspace.getConfiguration().get<Json>(message.config),
            });
          const dispose = vscode.workspace.onDidChangeConfiguration(e => {
            if (e.affectsConfiguration(message.config)) {
              sendLatestValue();
            }
          });
          sendLatestValue();
          onDispose(() => dispose.dispose());
          break;
        }
        case 'platform/executeVSCodeCommand': {
          vscode.commands.executeCommand(message.command, ...message.args);
          break;
        }
      }
    } catch (err) {
      vscode.window.showErrorMessage(`error handling message ${JSON.stringify(message)}\n${err}`);
    }
  },
});
