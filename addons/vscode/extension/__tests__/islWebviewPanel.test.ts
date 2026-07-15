/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Repository} from 'isl-server/src/Repository';
import {repositoryCache} from 'isl-server/src/RepositoryCache';
import * as vscode from 'vscode';
import {cwdForOpenISLCommand} from '../islWebviewPanel';

jest.mock('vscode', () => {
  const actualVscode = jest.requireActual('../../__mocks__/vscode');
  return {
    ...actualVscode,
  };
});

describe('cwdForOpenISLCommand', () => {
  const repoRoot = '/repo/root';

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('resolves the repo root from a SourceControl (scm/title button)', () => {
    // VS Code passes the repository's SourceControl when the button is clicked from its title bar.
    const sourceControl = {rootUri: vscode.Uri.file(repoRoot)} as vscode.SourceControl;
    expect(cwdForOpenISLCommand(sourceControl)).toBe(repoRoot);
  });

  it('resolves the repo root from a file Uri (editor/title button)', () => {
    const mockRepo = {info: {repoRoot}} as unknown as Repository;
    const spy = jest.spyOn(repositoryCache, 'cachedRepositoryForPath').mockReturnValue(mockRepo);

    const fileUri = vscode.Uri.file(`${repoRoot}/path/to/file.ts`);
    expect(cwdForOpenISLCommand(fileUri)).toBe(repoRoot);
    expect(spy).toHaveBeenCalledWith(fileUri.fsPath);
  });

  it('falls back to the Uri path when no repo is cached for it', () => {
    jest.spyOn(repositoryCache, 'cachedRepositoryForPath').mockReturnValue(undefined);

    const fileUri = vscode.Uri.file(`${repoRoot}/path/to/file.ts`);
    expect(cwdForOpenISLCommand(fileUri)).toBe(fileUri.fsPath);
  });

  it('returns undefined with no argument (keybinding)', () => {
    expect(cwdForOpenISLCommand(undefined)).toBeUndefined();
  });

  it('returns undefined for an argument without a rootUri', () => {
    expect(cwdForOpenISLCommand({})).toBeUndefined();
    expect(cwdForOpenISLCommand({rootUri: undefined})).toBeUndefined();
  });
});
