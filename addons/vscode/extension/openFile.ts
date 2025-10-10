/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Repository} from 'isl-server/src/Repository';
import type {AbsolutePath, RepoRelativePath} from 'isl/src/types';

import * as pathModule from 'node:path';
import * as vscode from 'vscode';
import {shouldOpenBeside} from './config';

const IMAGE_EXTENSIONS = new Set(['.bmp', '.gif', '.ico', '.jpeg', '.jpg', '.png', '.webp']);
function looksLikeImageUri(uri: vscode.Uri): boolean {
  const ext = pathModule.extname(uri.path).toLowerCase();
  return IMAGE_EXTENSIONS.has(ext);
}

/**
 * Opens a file in the editor within a provided repository, optionally at a specific line number.
 * The file path should be relative to the repository root.
 */
export function openFileInRepo(
  repo: Repository,
  filePath: RepoRelativePath,
  line?: number,
  preview?: boolean,
  onError?: (err: Error) => void,
  onOpened?: (editor: vscode.TextEditor) => void,
  disableScroll: boolean = false,
) {
  const path: AbsolutePath = pathModule.join(repo.info.repoRoot, filePath);
  openFileImpl(path, line, preview, onError, onOpened, disableScroll);
}

/**
 * Opens a file in the editor, optionally at a specific line number.
 * The file path should be absolute.
 */
export function openFile(
  filePath: AbsolutePath,
  line?: number,
  preview?: boolean,
  onError?: (err: Error) => void,
  onOpened?: (editor: vscode.TextEditor) => void,
  disableScroll: boolean = false,
): void {
  openFileImpl(filePath, line, preview, onError, onOpened, disableScroll);
}

function openFileImpl(
  filePath: AbsolutePath,
  line?: number,
  preview?: boolean,
  onError?: (err: Error) => void,
  onOpened?: (editor: vscode.TextEditor) => void,
  disableScroll: boolean = false,
): void {
  const uri = vscode.Uri.file(filePath);
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
        if (!disableScroll && line != null) {
          const lineZeroIndexed = line - 1; // vscode uses 0-indexed line numbers
          editor.selections = [new vscode.Selection(lineZeroIndexed, 0, lineZeroIndexed, 0)]; // move cursor to line
          editor.revealRange(
            new vscode.Range(lineZeroIndexed, 0, lineZeroIndexed, 0),
            vscode.TextEditorRevealType.InCenterIfOutsideViewport,
          ); // scroll to line
        }
        onOpened?.(editor);
      },
      err => {
        if (onError) {
          onError(err);
        } else {
          vscode.window.showErrorMessage(err.message ?? String(err));
        }
      },
    );
}
