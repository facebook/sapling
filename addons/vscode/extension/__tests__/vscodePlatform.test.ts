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
import {encodeSaplingDiffUri} from '../DiffContentProvider';
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

describe('platform/subscribeToVSCodeConfig', () => {
  const mockExtensionContext = {
    globalState: {
      update: jest.fn(),
    },
  } as unknown as vscode.ExtensionContext;

  beforeEach(() => {
    jest.clearAllMocks();
    jest.spyOn(vscode.workspace, 'getConfiguration').mockReturnValue({
      get: jest.fn().mockReturnValue('Auto'),
    } as never);
  });

  afterEach(() => {
    jest.restoreAllMocks();
  });

  it('still reports other message handling errors', async () => {
    const postMessage = jest.fn(() => {
      throw new Error('boom');
    });
    const platform = getVSCodePlatform(mockExtensionContext);

    const message: PlatformSpecificClientToServerMessages = {
      type: 'platform/subscribeToVSCodeConfig',
      config: 'sapling.comparisonPanelMode',
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

    expect(vscode.window.showErrorMessage).toHaveBeenCalledWith(
      expect.stringContaining('error handling message'),
    );
    expect(vscode.window.showErrorMessage).toHaveBeenCalledWith(expect.stringContaining('boom'));
  });
});

describe('platform/openFileAtRevset', () => {
  const mockExtensionContext = {
    globalState: {update: jest.fn()},
  } as unknown as vscode.ExtensionContext;

  const repoRoot = '/path/to/repo';
  const mockRepo = {info: {repoRoot}} as never;

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('opens the in-diff version via a read-only sapling-diff URI', async () => {
    const platform = getVSCodePlatform(mockExtensionContext);
    const message: PlatformSpecificClientToServerMessages = {
      type: 'platform/openFileAtRevset',
      path: 'src/file.ts',
      revset: 'abc123',
    };

    await platform.handleMessageFromClient.call(
      platform,
      mockRepo,
      mockCtx,
      message,
      jest.fn() as (msg: ServerToClientMessage) => void,
      jest.fn(),
      jest.fn(),
    );

    const expectedUri = encodeSaplingDiffUri(vscode.Uri.file(`${repoRoot}/src/file.ts`), 'abc123');
    expect(vscode.window.showTextDocument).toHaveBeenCalledWith(expectedUri, {
      viewColumn: undefined,
    });
  });

  it('does nothing when there is no repo', async () => {
    const platform = getVSCodePlatform(mockExtensionContext);
    const message: PlatformSpecificClientToServerMessages = {
      type: 'platform/openFileAtRevset',
      path: 'src/file.ts',
      revset: 'abc123',
    };

    await platform.handleMessageFromClient.call(
      platform,
      undefined,
      mockCtx,
      message,
      jest.fn() as (msg: ServerToClientMessage) => void,
      jest.fn(),
      jest.fn(),
    );

    expect(vscode.window.showTextDocument).not.toHaveBeenCalled();
  });
});
