/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepositoryContext} from 'isl-server/src/serverTypes';
import type {
  CwdInfo,
  PlatformSpecificClientToServerMessages,
  ServerToClientMessage,
} from 'isl/src/types';

import * as vscode from 'vscode';
import {getVSCodePlatform} from '../vscodePlatform';

jest.mock('isl-server/src/Repository', () => ({
  Repository: {
    getCwdInfo: jest.fn(),
  },
}));

// eslint-disable-next-line @typescript-eslint/no-var-requires, @typescript-eslint/no-require-imports
const {Repository} = require('isl-server/src/Repository');

const mockCtx: RepositoryContext = {
  cwd: '/test/cwd',
  cmd: 'sl',
  logger: {log: jest.fn(), info: jest.fn(), warn: jest.fn(), error: jest.fn()} as never,
  tracker: {context: {setRepo: jest.fn()}} as never,
};

describe('platform/subscribeToAvailableCwds', () => {
  const mockExtensionContext = {
    globalState: {
      update: jest.fn(),
    },
  } as unknown as vscode.ExtensionContext;

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('sends available cwds for all valid folders even when one getCwdInfo rejects', async () => {
    const goodCwd: CwdInfo = {cwd: '/path/to/repo1', repoRoot: '/path/to/repo1'};

    (vscode.workspace as {workspaceFolders?: vscode.WorkspaceFolder[]}).workspaceFolders = [
      {name: 'folder1', index: 0, uri: vscode.Uri.file('/path/to/repo1')},
      {name: 'folder2', index: 1, uri: vscode.Uri.file('/path/to/broken')},
    ];

    Repository.getCwdInfo.mockImplementation((ctx: RepositoryContext) => {
      if (ctx.cwd.includes('broken')) {
        return Promise.reject(new Error('simulated failure'));
      }
      return Promise.resolve(goodCwd);
    });

    const postMessage = jest.fn();
    const platform = getVSCodePlatform(mockExtensionContext);

    const message: PlatformSpecificClientToServerMessages = {
      type: 'platform/subscribeToAvailableCwds',
    } as PlatformSpecificClientToServerMessages;

    await platform.handleMessageFromClient.call(
      platform,
      undefined,
      mockCtx,
      message,
      postMessage as (msg: ServerToClientMessage) => void,
      jest.fn(),
      jest.fn(),
    );

    // Wait for the async postAllAvailableCwds to complete
    await new Promise(resolve => setTimeout(resolve, 0));

    expect(postMessage).toHaveBeenCalledWith({
      type: 'platform/availableCwds',
      options: [goodCwd],
    });
  });

  it('sends all available cwds when all getCwdInfo succeed', async () => {
    const cwd1: CwdInfo = {cwd: '/path/to/repo1', repoRoot: '/path/to/repo1'};
    const cwd2: CwdInfo = {cwd: '/path/to/repo2', repoRoot: '/path/to/repo2'};

    (vscode.workspace as {workspaceFolders?: vscode.WorkspaceFolder[]}).workspaceFolders = [
      {name: 'folder1', index: 0, uri: vscode.Uri.file('/path/to/repo1')},
      {name: 'folder2', index: 1, uri: vscode.Uri.file('/path/to/repo2')},
    ];

    Repository.getCwdInfo.mockResolvedValueOnce(cwd1).mockResolvedValueOnce(cwd2);

    const postMessage = jest.fn();
    const platform = getVSCodePlatform(mockExtensionContext);

    const message: PlatformSpecificClientToServerMessages = {
      type: 'platform/subscribeToAvailableCwds',
    } as PlatformSpecificClientToServerMessages;

    await platform.handleMessageFromClient.call(
      platform,
      undefined,
      mockCtx,
      message,
      postMessage as (msg: ServerToClientMessage) => void,
      jest.fn(),
      jest.fn(),
    );

    await new Promise(resolve => setTimeout(resolve, 0));

    expect(postMessage).toHaveBeenCalledWith({
      type: 'platform/availableCwds',
      options: [cwd1, cwd2],
    });
  });
});
