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
  Diagnostic,
  DiagnosticSeverity,
  PlatformSpecificClientToServerMessages,
  RepoRelativePath,
  ServerToClientMessage,
} from 'isl/src/types';
import type {Json} from 'shared/typeUtils';

import {Repository} from 'isl-server/src/Repository';
import {arraysEqual} from 'isl/src/utils';
import * as pathModule from 'node:path';
import * as vscode from 'vscode';
import {executeVSCodeCommand} from './commands';
import {PERSISTED_STORAGE_KEY_PREFIX} from './config';
import {t} from './i18n';
import {Internal} from './Internal';
import openFile from './openFile';
import {ActionTriggerType} from './types';

export type VSCodeServerPlatform = ServerPlatform & {
  panelOrView: undefined | vscode.WebviewPanel | vscode.WebviewView;
};

function diagnosticSeverity(severity: vscode.DiagnosticSeverity): DiagnosticSeverity {
  switch (severity) {
    case vscode.DiagnosticSeverity.Error:
      return 'error';
    case vscode.DiagnosticSeverity.Warning:
      return 'warning';
    case vscode.DiagnosticSeverity.Information:
      return 'info';
    case vscode.DiagnosticSeverity.Hint:
      return 'hint';
  }
}

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
            if (repo == null) {
              return;
            }
            // don't use preview mode for opening multiple files, since they would overwrite each other
            openFile(
              repo,
              path,
              message.options?.line,
              /* preview */ false,
              /* onError */ (err: Error) => {
                // Opening multiple files at once can throw errors even when the files are successfully opened
                // We check here if the error is unwarranted and the file actually exists in the tab group
                const uri = vscode.Uri.file(pathModule.join(repo.info.repoRoot, path));
                const isTabOpen = vscode.window.tabGroups.all
                  .flatMap(group => group.tabs)
                  .some(
                    tab =>
                      tab.input instanceof vscode.TabInputText &&
                      uri.fsPath == tab.input.uri.fsPath,
                  );

                if (!isTabOpen) {
                  vscode.window.showErrorMessage(err.message ?? String(err));
                }
              },
            );
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
        case 'platform/checkForDiagnostics': {
          const diagnosticMap = new Map<RepoRelativePath, Array<Diagnostic>>();
          const repoRoot = repo?.info.repoRoot;
          if (repoRoot) {
            for (const path of message.paths) {
              const uri = vscode.Uri.file(pathModule.join(repoRoot, path));
              const diagnostics = vscode.languages.getDiagnostics(uri);
              if (diagnostics.length > 0) {
                diagnosticMap.set(
                  path,
                  diagnostics.map(diagnostic => ({
                    message: diagnostic.message,
                    range: {
                      startLine: diagnostic.range.start.line,
                      startCol: diagnostic.range.start.character,
                      endLine: diagnostic.range.end.line,
                      endCol: diagnostic.range.end.character,
                    },
                    severity: diagnosticSeverity(diagnostic.severity),
                    source: diagnostic.source,
                    code:
                      typeof diagnostic.code === 'object'
                        ? String(diagnostic.code.value)
                        : String(diagnostic.code),
                  })),
                );
              }
            }
          }
          postMessage({type: 'platform/gotDiagnostics', diagnostics: diagnosticMap});
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
        case 'platform/subscribeToSuggestedEdits': {
          const dispose = Internal.suggestedEdits?.onDidChangeSuggestedEdits(
            (files: Array<AbsolutePath>) => {
              postMessage({
                type: 'platform/onDidChangeSuggestedEdits',
                files,
              });
            },
          );
          onDispose(() => dispose?.dispose());
          break;
        }
        case 'platform/resolveSuggestedEdits': {
          Internal.suggestedEdits?.resolveSuggestedEdits(message.action, message.files);
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
        case 'platform/resolveAllCommentsWithAI': {
          const {diffId, comments, filePaths, repoPath} = message;
          Internal.promptAIAgent?.(
            {type: 'resolveAllComments', diffId, comments, filePaths, repoPath},
            ActionTriggerType.ISL2SmartActions,
          );
          break;
        }
        case 'platform/resolveFailedSignalsWithAI': {
          const {diffId, repoPath} = message;
          Internal.promptAIAgent?.(
            {type: 'resolveFailedSignals', diffId, repoPath},
            ActionTriggerType.ISL2SmartActions,
          );
          break;
        }
        case 'platform/fillDevmateCommitMessage': {
          const {source} = message;
          // Call Devmate to generate a commit message based on the current changes
          Internal.promptAIAgent?.(
            {type: 'fillCommitMessage'},
            source === 'commitInfoView'
              ? ActionTriggerType.ISL2CommitInfoView
              : ActionTriggerType.ISL2SmartActions,
          );
          break;
        }
        case 'platform/devmateCreateTestForModifiedCode': {
          Internal.promptTestGeneration?.();
          break;
        }
        case 'platform/setFirstPassCodeReviewDiagnostics': {
          const {issueMap} = message;
          for (const filePath of issueMap.keys()) {
            Internal.firstPassCodeReviewDiagnosticsProvider?.().setCodeReviewDiagnostics(
              filePath,
              issueMap.get(filePath) ?? [],
            );
          }
          break;
        }
        case 'platform/devmateValidateChanges': {
          Internal.promptAIAgent?.({type: 'validateChanges'}, ActionTriggerType.ISL2SmartActions);
          break;
        }
        case 'platform/devmateResolveAllConflicts': {
          const {conflicts} = message;
          Internal.promptAIAgent?.(
            {type: 'resolveAllConflicts', conflicts, repoPath: repo?.info.repoRoot},
            ActionTriggerType.ISL2MergeConflictView,
          );
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
