/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {generatedFilesDetector} from 'isl-server/src/GeneratedFiles';
import type {Repository} from 'isl-server/src/Repository';
import {repositoryCache} from 'isl-server/src/RepositoryCache';
import {GeneratedStatus} from 'isl/src/types';
import * as vscode from 'vscode';
import {encodeSaplingDiffUri} from '../DiffContentProvider';
import {cwdForOpenISLCommand, initialCwdForISL, sortGeneratedFilesToEnd} from '../islWebviewPanel';

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

  it('resolves the repo root from a sapling-diff Uri (diff view)', () => {
    const mockRepo = {info: {repoRoot}} as unknown as Repository;
    jest.spyOn(repositoryCache, 'cachedRepositoryForPath').mockReturnValue(mockRepo);

    const diffUri = encodeSaplingDiffUri(vscode.Uri.file(`${repoRoot}/path/to/file.ts`), 'abc123');
    expect(cwdForOpenISLCommand(diffUri)).toBe(repoRoot);
  });

  it('returns undefined for a webview Uri (webview tab, e.g. the Home Page)', () => {
    jest.spyOn(repositoryCache, 'cachedRepositoryForPath').mockReturnValue(undefined);

    // A webview tab's resource Uri path is a panel id, not a folder.
    const webviewUri = vscode.Uri.from({
      scheme: 'webview-panel',
      path: 'webview-panel/webview-Home Page-aae7a9ea-c65c-4592-8ff5-af1fd255020f',
    });
    expect(cwdForOpenISLCommand(webviewUri)).toBeUndefined();
  });

  it('returns undefined with no argument (keybinding)', () => {
    expect(cwdForOpenISLCommand(undefined)).toBeUndefined();
  });

  it('returns undefined for an argument without a rootUri', () => {
    expect(cwdForOpenISLCommand({})).toBeUndefined();
    expect(cwdForOpenISLCommand({rootUri: undefined})).toBeUndefined();
  });
});

describe('initialCwdForISL', () => {
  it('reopens with the most recently selected cwd', () => {
    expect(
      initialCwdForISL(
        undefined,
        undefined,
        '/workspace/second',
        '/workspace/first',
        '/process/cwd',
      ),
    ).toBe('/workspace/second');
  });

  it('prefers an explicitly requested cwd', () => {
    expect(
      initialCwdForISL(
        '/workspace/requested',
        undefined,
        '/workspace/recent',
        '/workspace/first',
        '/process/cwd',
      ),
    ).toBe('/workspace/requested');
  });

  it('prefers a focused environment over the remembered cwd', () => {
    expect(
      initialCwdForISL(
        undefined,
        '/workspace/focused',
        '/workspace/recent',
        '/workspace/first',
        '/process/cwd',
      ),
    ).toBe('/workspace/focused');
  });
});

describe('sortGeneratedFilesToEnd', () => {
  const repoRoot = '/repo/root';
  const mockRepo = {
    info: {repoRoot},
    initialConnectionContext: {},
    getConfig: jest.fn(),
  } as unknown as Repository;

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('moves generated files to the end while preserving relative order', async () => {
    jest.spyOn(generatedFilesDetector, 'queryFilesGenerated').mockResolvedValue({
      'gen-a.ts': GeneratedStatus.Generated,
      'manual-a.ts': GeneratedStatus.Manual,
      'gen-b.ts': GeneratedStatus.Generated,
      'manual-b.ts': GeneratedStatus.Manual,
      'partial.ts': GeneratedStatus.PartiallyGenerated,
    });
    const files = [
      {path: 'gen-a.ts'},
      {path: 'manual-a.ts'},
      {path: 'gen-b.ts'},
      {path: 'manual-b.ts'},
      {path: 'partial.ts'},
    ];
    await expect(sortGeneratedFilesToEnd(mockRepo, files)).resolves.toEqual([
      {path: 'manual-a.ts'},
      {path: 'manual-b.ts'},
      {path: 'partial.ts'},
      {path: 'gen-a.ts'},
      {path: 'gen-b.ts'},
    ]);
  });

  it('orders manual first, then partially generated, then generated', async () => {
    jest.spyOn(generatedFilesDetector, 'queryFilesGenerated').mockResolvedValue({
      'gen.ts': GeneratedStatus.Generated,
      'manual.ts': GeneratedStatus.Manual,
      'partial.ts': GeneratedStatus.PartiallyGenerated,
    });
    const files = [{path: 'gen.ts'}, {path: 'partial.ts'}, {path: 'manual.ts'}];
    await expect(sortGeneratedFilesToEnd(mockRepo, files)).resolves.toEqual([
      {path: 'manual.ts'},
      {path: 'partial.ts'},
      {path: 'gen.ts'},
    ]);
  });

  it('treats unknown statuses as manual', async () => {
    jest.spyOn(generatedFilesDetector, 'queryFilesGenerated').mockResolvedValue({
      'gen.ts': GeneratedStatus.Generated,
    });
    const files = [{path: 'gen.ts'}, {path: 'unknown.ts'}];
    await expect(sortGeneratedFilesToEnd(mockRepo, files)).resolves.toEqual([
      {path: 'unknown.ts'},
      {path: 'gen.ts'},
    ]);
  });
  it('leaves order unchanged when statuses cannot be determined', async () => {
    jest.spyOn(generatedFilesDetector, 'queryFilesGenerated').mockRejectedValue(new Error('boom'));
    const files = [{path: 'gen.ts'}, {path: 'manual.ts'}];
    await expect(sortGeneratedFilesToEnd(mockRepo, files)).resolves.toEqual(files);
  });
});
