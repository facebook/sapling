/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Set as ImSet} from 'immutable';
import type {Repository} from 'isl-server/src/Repository';
import {repositoryCache} from 'isl-server/src/RepositoryCache';
import fs from 'node:fs';
import {ComparisonType, type Comparison} from 'shared/Comparison';
import * as vscode from 'vscode';
import {vscodeCommands} from '../commands';
import {shouldOpenBeside} from '../config';
import {encodeDeletedFileUri} from '../DeletedFileContentProvider';
import {encodeSaplingDiffUri} from '../DiffContentProvider';

// Mock vscode command
jest.mock('vscode', () => {
  const actualVscode = jest.requireActual('../../__mocks__/vscode');
  return {
    ...actualVscode,
    commands: {
      executeCommand: jest.fn(),
    },
  };
});
const mockExecuteVSCodeCommand = vscode.commands.executeCommand as jest.MockedFunction<
  typeof vscode.commands.executeCommand
>;

// Mock fs access
jest.mock('node:fs', () => ({
  promises: {
    access: jest.fn(),
  },
}));
const mockFsAccess = fs.promises.access as jest.MockedFunction<typeof fs.promises.access>;

// Mock global config
jest.mock('../config', () => ({
  shouldOpenBeside: jest.fn(),
}));
const mockShouldOpenBeside = shouldOpenBeside as jest.MockedFunction<typeof shouldOpenBeside>;

describe('open-file-diff', () => {
  const openDiffView = vscodeCommands['sapling.open-file-diff'];

  const repoRoot = '/repo/root';
  const filePath = 'path/to/file';
  const submodulePath = 'path/to/submodule';
  const fileUri = vscode.Uri.file(`${repoRoot}/${filePath}`);
  const submoduleUri = vscode.Uri.file(`${repoRoot}/${submodulePath}`);

  // Create a proper mock repository
  const mockRepo = {
    info: {
      repoRoot,
    },
    getSubmodulePathCache: jest.fn(),
  } as unknown as jest.Mocked<Repository>;

  beforeEach(() => {
    jest.clearAllMocks();

    jest.spyOn(repositoryCache, 'cachedRepositoryForPath').mockReturnValue(mockRepo);
    mockRepo.getSubmodulePathCache.mockReturnValue(ImSet([submodulePath]));
    mockShouldOpenBeside.mockReturnValue(false);
  });

  it('uncommitted changes, regular file', async () => {
    mockFsAccess.mockResolvedValue(undefined); // File exists

    const comparison: Comparison = {type: ComparisonType.UncommittedChanges};
    await openDiffView(fileUri, comparison);

    const expectedLeftRev = '.';
    const expectedLeftUri = encodeSaplingDiffUri(fileUri, expectedLeftRev);

    expect(mockExecuteVSCodeCommand).toHaveBeenCalledWith(
      'vscode.diff',
      expectedLeftUri,
      fileUri,
      'file (Uncommitted Changes)',
      {viewColumn: undefined},
    );
  });

  it('uncommitted changes, submodule', async () => {
    mockFsAccess.mockRejectedValue(undefined); // Path exists

    const comparison: Comparison = {type: ComparisonType.UncommittedChanges};
    await openDiffView(submoduleUri, comparison);

    const expectedLeftRev = '.';
    const expectedLeftUri = encodeSaplingDiffUri(submoduleUri, expectedLeftRev);
    const expectedRightRev = 'wdir()';
    const expectedRightUri = encodeSaplingDiffUri(submoduleUri, expectedRightRev);

    expect(mockExecuteVSCodeCommand).toHaveBeenCalledWith(
      'vscode.diff',
      expectedLeftUri,
      expectedRightUri,
      'submodule (Uncommitted Changes)',
      {viewColumn: undefined},
    );
  });

  it('uncommitted changes, file deleted', async () => {
    mockFsAccess.mockRejectedValue(new Error('File not found')); // File doesn't exist

    const comparison: Comparison = {type: ComparisonType.UncommittedChanges};
    await openDiffView(fileUri, comparison);

    const expectedLeftRev = '.';
    const expectedLeftUri = encodeSaplingDiffUri(fileUri, expectedLeftRev);
    const expectedRightUri = encodeDeletedFileUri(fileUri);

    expect(mockExecuteVSCodeCommand).toHaveBeenCalledWith(
      'vscode.diff',
      expectedLeftUri,
      expectedRightUri,
      'file (Uncommitted Changes)',
      {viewColumn: undefined},
    );
  });

  it('head changes, regular file', async () => {
    mockFsAccess.mockResolvedValue(undefined); // File exists

    const comparison: Comparison = {type: ComparisonType.HeadChanges};
    await openDiffView(fileUri, comparison);

    const expectedLeftRev = '.^';
    const expectedLeftUri = encodeSaplingDiffUri(fileUri, expectedLeftRev);

    expect(mockExecuteVSCodeCommand).toHaveBeenCalledWith(
      'vscode.diff',
      expectedLeftUri,
      fileUri,
      'file (Head Changes)',
      {viewColumn: undefined},
    );
  });

  it('head changes, submodule', async () => {
    mockFsAccess.mockRejectedValue(undefined); // Path exists

    const comparison: Comparison = {type: ComparisonType.HeadChanges};
    await openDiffView(submoduleUri, comparison);

    const expectedLeftRev = '.^';
    const expectedLeftUri = encodeSaplingDiffUri(submoduleUri, expectedLeftRev);
    const expectedRightRev = 'wdir()';
    const expectedRightUri = encodeSaplingDiffUri(submoduleUri, expectedRightRev);

    expect(mockExecuteVSCodeCommand).toHaveBeenCalledWith(
      'vscode.diff',
      expectedLeftUri,
      expectedRightUri,
      'submodule (Head Changes)',
      {viewColumn: undefined},
    );
  });

  it('stack changes, regular file', async () => {
    mockFsAccess.mockResolvedValue(undefined); // File exists

    const comparison: Comparison = {type: ComparisonType.StackChanges};
    await openDiffView(fileUri, comparison);

    const expectedLeftRev = 'ancestor(.,interestingmaster())';
    const expectedLeftUri = encodeSaplingDiffUri(fileUri, expectedLeftRev);

    expect(mockExecuteVSCodeCommand).toHaveBeenCalledWith(
      'vscode.diff',
      expectedLeftUri,
      fileUri,
      'file (Stack Changes)',
      {viewColumn: undefined},
    );
  });

  it('stack changes, submodule', async () => {
    mockFsAccess.mockRejectedValue(undefined); // Path exists

    const comparison: Comparison = {type: ComparisonType.StackChanges};
    await openDiffView(submoduleUri, comparison);

    const expectedLeftRev = 'ancestor(.,interestingmaster())';
    const expectedLeftUri = encodeSaplingDiffUri(submoduleUri, expectedLeftRev);
    const expectedRightRev = 'wdir()';
    const expectedRightUri = encodeSaplingDiffUri(submoduleUri, expectedRightRev);

    expect(mockExecuteVSCodeCommand).toHaveBeenCalledWith(
      'vscode.diff',
      expectedLeftUri,
      expectedRightUri,
      'submodule (Stack Changes)',
      {viewColumn: undefined},
    );
  });

  it('committed changes, regular file', async () => {
    const comparison: Comparison = {type: ComparisonType.Committed, hash: 'abc123'};
    await openDiffView(fileUri, comparison);

    const expectedLeftRev = 'abc123^';
    const expectedLeftUri = encodeSaplingDiffUri(fileUri, expectedLeftRev);
    const expectedRightRev = 'abc123';
    const expectedRightUri = encodeSaplingDiffUri(fileUri, expectedRightRev);

    expect(mockExecuteVSCodeCommand).toHaveBeenCalledWith(
      'vscode.diff',
      expectedLeftUri,
      expectedRightUri,
      'file (In abc123)',
      {viewColumn: undefined},
    );
  });

  it('committed changes, submodule', async () => {
    const comparison: Comparison = {type: ComparisonType.Committed, hash: 'abc123'};
    await openDiffView(submoduleUri, comparison);

    const expectedLeftRev = 'abc123^';
    const expectedLeftUri = encodeSaplingDiffUri(submoduleUri, expectedLeftRev);
    const expectedRightRev = 'abc123';
    const expectedRightUri = encodeSaplingDiffUri(submoduleUri, expectedRightRev);

    expect(mockExecuteVSCodeCommand).toHaveBeenCalledWith(
      'vscode.diff',
      expectedLeftUri,
      expectedRightUri,
      'submodule (In abc123)',
      {viewColumn: undefined},
    );
  });
});
