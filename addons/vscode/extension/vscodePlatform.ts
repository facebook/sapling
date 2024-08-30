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
import {PERSISTED_STORAGE_KEY_PREFIX, shouldOpenBeside} from './config';
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

function openFile(
  repo: Repository | undefined,
  filePath: string,
  line?: number,
  preview?: boolean,
) {
  if (repo == null) {
    return;
  }
  const path: AbsolutePath = pathModule.join(repo.info.repoRoot, filePath);
  const uri = vscode.Uri.file(path);
  if (looksLikeImageUri(uri)) {
    vscode.commands.executeCommand('vscode.open', uri).then(undefined, err => {
      vscode.window.showErrorMessage('cannot open file' + (err.message ?? String(err)));
    });
    return;
  }
  vscode.window
    .showTextDocument(uri, {
      preview,
      viewColumn: shouldOpenBeside() ? vscode.ViewColumn.Beside : undefined,
    })
    .then(
      editor => {
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
}
export type VSCodeServerPlatform = ServerPlatform & {
  panelOrView: undefined | vscode.WebviewPanel | vscode.WebviewView;
};

export const getVSCodePlatform = (context: vscode.ExtensionContext): VSCodeServerPlatform => ({
  platformName: 'vscode',
  sessionId: vscode.env.sessionId,
  panelOrView: undefined,
  async handleMessageFromClient(
    this: VSCodeServerPlatform,
    repo: Repository | undefined,
    ctx: RepositoryContext,
    message: PlatformSpecificClientToServerMessages,
    postMessage: (message: ServerToClientMessage) => void,
    onDispose: (cb: () => unknown) => void,
  ) {
    try {
      switch (message.type) {
        case 'platform/openFiles': {
          for (const path of message.paths) {
            // don't use preview mode for opening multiple files, since they would overwrite each other
            openFile(repo, path, message.options?.line, /* preview */ false);
          }
          break;
        }
        case 'platform/openFile': {
          openFile(repo, message.path, message.options?.line, undefined);
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
        case 'platform/changeTitle': {
          if (this.panelOrView != null) {
            this.panelOrView.title = message.title;
          }
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
            const files = getUnsavedFiles(repo).map(document => {
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
        case 'platform/saveAllUnsavedFiles': {
          if (repo == null) {
            return;
          }
          Promise.all(getUnsavedFiles(repo).map(doc => doc.save())).then(results => {
            postMessage({
              type: 'platform/savedAllUnsavedFiles',
              success: results.every(result => result),
            });
          });
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
          const {key, data} = message;
          context.globalState.update(PERSISTED_STORAGE_KEY_PREFIX + key, data);
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

function getUnsavedFiles(repo: Repository): Array<vscode.TextDocument> {
  return vscode.workspace.textDocuments.filter(
    document => document.isDirty && repo.isPathInsideRepo(document.fileName),
  );
}
