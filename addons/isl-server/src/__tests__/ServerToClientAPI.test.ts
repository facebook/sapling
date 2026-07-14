/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepoInfo, ValidatedRepoInfo} from 'isl/src/types';
import type {ClientConnection} from '..';
import type {ServerPlatform} from '../serverPlatform';
import type {RepositoryContext} from '../serverTypes';

import {deserializeFromString, serializeToString} from 'isl/src/serialize';
import {mockLogger, nextTick} from 'shared/testUtils';
import {Internal} from '../Internal';
import {Repository} from '../Repository';
import {repositoryCache} from '../RepositoryCache';
import ServerToClientAPI from '../ServerToClientAPI';
import {makeServerSideTracker} from '../analytics/serverSideTracker';

jest.mock('../RepositoryCache', () => {
  const original = jest.requireActual('../RepositoryCache');
  return {
    ...original,
    repositoryCache: {
      getOrCreate: jest.fn(() => ({
        promise: Promise.resolve(mockRepo),
        unref: jest.fn(),
      })),
    },
  };
});

const mockRepoInfo: ValidatedRepoInfo = {
  type: 'success',
  repoRoot: '/path/to/repo',
  dotdir: '/path/to/repo/.sl',
  command: 'sl',
  preferredSubmitCommand: 'pr',
  codeReviewSystem: {type: 'unknown'},
  pullRequestDomain: undefined,
  isEdenFs: false,
};

class MockRepository {
  static getRepoInfo(_ctx: RepositoryContext): Promise<RepoInfo> {
    return Promise.resolve(mockRepoInfo);
  }
  info = mockRepoInfo;
  codeReviewProvider = null;
  ref = jest.fn();
  unref = jest.fn();
  dispose = jest.fn();
  fetchAndSetRecommendedBookmarks = jest.fn();
  fetchAndSetHiddenMasterConfig = jest.fn();
  initialConnectionContext = {logger: mockLogger};
}

const mockRepo = new MockRepository();

const mockTracker = makeServerSideTracker(
  mockLogger,
  {platformName: 'test'} as ServerPlatform,
  '0.1',
  jest.fn(),
);

type MessageHandler = (event: Buffer, isBinary: boolean) => void | Promise<void>;

function createMockConnection(cwd = '/path/to/repo/cwd'): ClientConnection & {
  triggerMessage: (msg: Record<string, unknown>) => void;
} {
  let handler: MessageHandler | undefined;
  return {
    postMessage: jest.fn().mockResolvedValue(true),
    onDidReceiveMessage: jest.fn((cb: MessageHandler) => {
      handler = cb;
      return {dispose: jest.fn()};
    }),
    command: 'sl',
    version: '0.1',
    cwd,
    appMode: {mode: 'isl'},
    triggerMessage(msg: Record<string, unknown>) {
      handler?.(Buffer.from(serializeToString(msg as never)), false);
    },
  };
}

describe('ServerToClientAPI disposable scoping', () => {
  let repoDispose: jest.Mock;
  let connectionDispose: jest.Mock;
  let platform: ServerPlatform;
  let connection: ReturnType<typeof createMockConnection>;
  let api: ServerToClientAPI;

  beforeEach(async () => {
    repoDispose = jest.fn();
    connectionDispose = jest.fn();

    platform = {
      platformName: 'test',
      handleMessageFromClient(_repo, _ctx, _message, _postMessage, onDispose, onConnectionDispose) {
        onDispose(() => repoDispose());
        onConnectionDispose?.(() => connectionDispose());
      },
    };

    connection = createMockConnection();
    api = new ServerToClientAPI(platform, connection, mockTracker, mockLogger);
    api.setActiveRepoForCwd('/path/to/repo/cwd');
    await nextTick();
  });

  afterEach(() => {
    api.dispose();
    jest.clearAllMocks();
  });

  it('disposes repo-scoped platform disposables on CWD change', async () => {
    // Send a platform message to register disposables
    connection.triggerMessage({type: 'platform/openExternal', url: 'https://example.com'});
    await nextTick();

    expect(repoDispose).not.toHaveBeenCalled();

    // Trigger a CWD change which calls disposeRepoDisposables
    api.setActiveRepoForCwd('/path/to/repo/other');
    await nextTick();

    expect(repoDispose).toHaveBeenCalledTimes(1);
  });

  it('preserves connection-scoped platform disposables across CWD changes', async () => {
    // Send a platform message to register disposables
    connection.triggerMessage({type: 'platform/openExternal', url: 'https://example.com'});
    await nextTick();

    expect(connectionDispose).not.toHaveBeenCalled();

    // Trigger a CWD change
    api.setActiveRepoForCwd('/path/to/repo/other');
    await nextTick();

    // Connection-scoped disposable should NOT be disposed on CWD change
    expect(connectionDispose).not.toHaveBeenCalled();
  });

  it('disposes connection-scoped disposables on full dispose', async () => {
    // Send a platform message to register disposables
    connection.triggerMessage({type: 'platform/openExternal', url: 'https://example.com'});
    await nextTick();

    expect(connectionDispose).not.toHaveBeenCalled();

    // Full dispose should clean up connection-scoped disposables
    api.dispose();

    expect(connectionDispose).toHaveBeenCalledTimes(1);
  });
});

describe('ServerToClientAPI QE flags without an Internal implementation (OSS)', () => {
  let platform: ServerPlatform;
  let connection: ReturnType<typeof createMockConnection>;
  let api: ServerToClientAPI;
  let originalFetchQeFlag: typeof Internal.fetchQeFlag;

  beforeEach(async () => {
    // Internal builds do provide Internal.fetchQeFlag, so force the OSS shape
    // (no implementation) to exercise the always-reply fallback.
    originalFetchQeFlag = Internal.fetchQeFlag;
    Internal.fetchQeFlag = undefined;

    platform = {
      platformName: 'test',
      handleMessageFromClient: jest.fn(),
    };

    // `fetchQeFlag` is only handled once the connection has an active repo, which
    // requires the resolved value to pass `instanceof Repository`. Give a plain object
    // Repository's prototype so the connection reaches the "repo" state.
    const repo = Object.assign(Object.create(Repository.prototype), {
      info: mockRepoInfo,
      initialConnectionContext: {logger: mockLogger},
      codeReviewProvider: null,
      ref: jest.fn(),
      unref: jest.fn(),
      dispose: jest.fn(),
      fetchAndSetRecommendedBookmarks: jest.fn(),
      fetchAndSetHiddenMasterConfig: jest.fn(),
      pullFetchedDiffs: jest.fn().mockResolvedValue(undefined),
    });
    (repositoryCache.getOrCreate as jest.Mock).mockReturnValue({
      promise: Promise.resolve(repo),
      unref: jest.fn(),
    });

    connection = createMockConnection();
    api = new ServerToClientAPI(platform, connection, mockTracker, mockLogger);
    api.setActiveRepoForCwd('/path/to/repo/cwd');
    await nextTick();
  });

  afterEach(() => {
    api.dispose();
    Internal.fetchQeFlag = originalFetchQeFlag;
    jest.clearAllMocks();
  });

  function responsesOfType(type: string): Array<Record<string, unknown>> {
    return (connection.postMessage as jest.Mock).mock.calls
      .map(([str]) => deserializeFromString(str as string) as Record<string, unknown>)
      .filter(msg => msg.type === type);
  }

  it('replies to fetchQeFlag with passes=false when Internal.fetchQeFlag is unavailable', async () => {
    // OSS builds have no Internal.fetchQeFlag; the server must still answer,
    // otherwise the client (e.g. gotoAction) awaits a reply that never comes.
    expect(Internal.fetchQeFlag).toBeUndefined();

    connection.triggerMessage({type: 'fetchQeFlag', name: 'isl_rebase_onto_warm_button'});
    await nextTick();

    expect(responsesOfType('fetchedQeFlag')).toEqual([
      {type: 'fetchedQeFlag', name: 'isl_rebase_onto_warm_button', passes: false},
    ]);
  });
});
