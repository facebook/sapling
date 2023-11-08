/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as vscode from 'vscode';

export const DELETED_FILE_DIFF_VIEW_PROVIDER_SCHEME = 'sapling-deleted-file';

/**
 * Provides empty content for deleted files, so they may be used in diff views.
 */
export class DeletedFileContentProvider implements vscode.TextDocumentContentProvider {
  disposable: vscode.Disposable;

  constructor() {
    this.disposable = vscode.workspace.registerTextDocumentContentProvider(
      DELETED_FILE_DIFF_VIEW_PROVIDER_SCHEME,
      this,
    );
  }

  public provideTextDocumentContent(_uri: vscode.Uri): Promise<string | null> {
    return Promise.resolve('');
  }

  dispose() {
    this.disposable.dispose();
  }
}

/**
 * URIs are provided for the QuickDiff encoded, so that we can restore the original URI.
 */
export function encodeDeletedFileUri(uri: vscode.Uri): vscode.Uri {
  return uri.with({
    scheme: DELETED_FILE_DIFF_VIEW_PROVIDER_SCHEME,
    query: JSON.stringify({originalScheme: uri.scheme}),
  });
}
