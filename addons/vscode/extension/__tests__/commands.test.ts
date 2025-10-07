/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

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
  const fileUri = vscode.Uri.file(`${repoRoot}/${filePath}`);

  beforeEach(() => {
    jest.clearAllMocks();
    mockShouldOpenBeside.mockReturnValue(false);
  });

  it('uncommitted changes, regular file', async () => {
    mockFsAccess.mockResolvedValue(undefined); // File exists

    const comparison: Comparison = {type: ComparisonType.UncommittedChanges};
    await openDiffView(fileUri, comparison);

    const expectedLeftUri = encodeSaplingDiffUri(fileUri, comparison);

    expect(mockExecuteVSCodeCommand).toHaveBeenCalledWith(
      'vscode.diff',
      expectedLeftUri,
      fileUri,
      'file (Uncommitted Changes)',
      {viewColumn: undefined},
    );
  });

  it('uncommitted changes, file deleted', async () => {
    mockFsAccess.mockRejectedValue(new Error('File not found')); // File doesn't exist

    const comparison: Comparison = {type: ComparisonType.UncommittedChanges};
    await openDiffView(fileUri, comparison);

    const expectedLeftUri = encodeSaplingDiffUri(fileUri, comparison);
    const expectedRightUri = encodeDeletedFileUri(fileUri);

    expect(mockExecuteVSCodeCommand).toHaveBeenCalledWith(
      'vscode.diff',
      expectedLeftUri,
      expectedRightUri,
      'file (Uncommitted Changes)',
      {viewColumn: undefined},
    );
  });

  it('head changes', async () => {
    mockFsAccess.mockResolvedValue(undefined); // File exists

    const comparison: Comparison = {type: ComparisonType.HeadChanges};
    await openDiffView(fileUri, comparison);

    const expectedLeftUri = encodeSaplingDiffUri(fileUri, comparison);

    expect(mockExecuteVSCodeCommand).toHaveBeenCalledWith(
      'vscode.diff',
      expectedLeftUri,
      fileUri,
      'file (Head Changes)',
      {viewColumn: undefined},
    );
  });

  it('stack changes', async () => {
    mockFsAccess.mockResolvedValue(undefined); // File exists

    const comparison: Comparison = {type: ComparisonType.StackChanges};
    await openDiffView(fileUri, comparison);

    const expectedLeftUri = encodeSaplingDiffUri(fileUri, comparison);

    expect(mockExecuteVSCodeCommand).toHaveBeenCalledWith(
      'vscode.diff',
      expectedLeftUri,
      fileUri,
      'file (Stack Changes)',
      {viewColumn: undefined},
    );
  });

  it('committed changes', async () => {
    const comparison: Comparison = {type: ComparisonType.Committed, hash: 'abc123'};
    await openDiffView(fileUri, comparison);

    const expectedRightUri = encodeSaplingDiffUri(fileUri, comparison);
    const expectedLeftUri = encodeSaplingDiffUri(fileUri, {
      type: ComparisonType.Committed,
      hash: 'abc123^',
    });

    expect(mockExecuteVSCodeCommand).toHaveBeenCalledWith(
      'vscode.diff',
      expectedLeftUri,
      expectedRightUri,
      'file (In abc123)',
    );
  });
});
