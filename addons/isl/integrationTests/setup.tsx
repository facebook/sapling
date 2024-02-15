/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {MessageBusStatus} from '../src/MessageBus';
import type {Disposable, RepoRelativePath} from '../src/types';
import type {ExecaChildProcess, Options as ExecaOptions} from 'execa';
import type {TypedEventEmitter} from 'shared/TypedEventEmitter';

import {onClientConnection} from '../../isl-server/src/index';
import App from '../src/App';
import mockedClientMessagebus from '../src/MessageBus';
import * as internalLogger from '../src/logger';
import {render} from '@testing-library/react';
import fs from 'fs';
import {runCommand} from 'isl-server/src/commands';
import os from 'os';
import path from 'path';
import React from 'react';

const mockLogger = internalLogger.logger;
const {log} = mockLogger;
jest.mock('../src/logger', () => {
  const log =
    process.argv.includes('--verbose') || process.argv.includes('-V')
      ? (...args: Parameters<typeof console.log>) => {
          // eslint-disable-next-line no-console
          console.log(...args);
        }
      : (() => {
          return () => undefined;
        })();
  return {
    logger: {
      log,
      info: log,
      warn: log,
      error: log,
    },
  };
});

// fake client message bus that connects to server in the same process
jest.mock('../src/MessageBus', () => {
  const {TypedEventEmitter} =
    // this mock implementation is hoisted above all other imports, so we can't use imports "normally"
    // eslint-disable-next-line @typescript-eslint/no-var-requires,@typescript-eslint/consistent-type-imports
    require('shared/TypedEventEmitter') as typeof import('shared/TypedEventEmitter');

  class IntegrationTestMessageBus {
    disposables: Array<() => void> = [];
    onMessage(handler: (event: MessageEvent<string>) => void | Promise<void>): Disposable {
      const cb = (message: string) => {
        log('<--', message);
        handler({data: message} as MessageEvent<string>);
      };

      this.serverToClient.on('data', cb);
      return {
        dispose: () => {
          this.serverToClient.off('data', cb);
        },
      };
    }

    postMessage(message: string | ArrayBuffer) {
      log('-->', message);
      this.clientToServer.emit('data', message as string);
    }

    public statusChangeHandlers = new Set<(status: MessageBusStatus) => unknown>();
    onChangeStatus(handler: (status: MessageBusStatus) => unknown): Disposable {
      // pretend connection opens immediately
      handler({type: 'open'});
      this.statusChangeHandlers.add(handler);

      return {
        dispose: () => {
          this.statusChangeHandlers.delete(handler);
        },
      };
    }

    /**** extra methods for testing ****/

    clientToServer = new TypedEventEmitter<'data', string | ArrayBuffer>();
    serverToClient = new TypedEventEmitter<'data', string>();

    dispose = () => {
      this.clientToServer.removeAllListeners();
      this.serverToClient.removeAllListeners();
    };
  }

  return new IntegrationTestMessageBus();
});

type MockedClientMessageBus = {
  clientToServer: TypedEventEmitter<'data', string | ArrayBuffer>;
  serverToClient: TypedEventEmitter<'data', string>;
  dispose(): void;
};

/**
 * Creates an sl repository in a temp dir on disk,
 * creates a single initial commit,
 * then performs an initial render, running both server and client in the same process.
 */
export async function initRepo(): Promise<{
  repoDir: string;
  sl: (args: Array<string>) => ExecaChildProcess;
  cleanup: () => Promise<void>;
  writeFileInRepo: (path: RepoRelativePath, content: string) => Promise<void>;
  drawdag: (dag: string) => Promise<void>;
}> {
  const repoDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'isl-integration-test-repo-'));
  log('temp repo: ', repoDir);

  function sl(args: Array<string>, options?: ExecaOptions) {
    return runCommand('sl', args, mockLogger, repoDir, {
      ...options,
      env: {
        ...(options?.env ?? {}),
        FB_SCM_DIAGS_NO_SCUBA: '1',
      } as Record<string, string> as NodeJS.ProcessEnv,
      extendEnv: true,
    });
  }

  async function writeFileInRepo(filePath: RepoRelativePath, content: string): Promise<void> {
    await fs.promises.writeFile(path.join(repoDir, filePath), content, 'utf8');
  }

  /**
   * create test commit history from a diagram.
   * See https://sapling-scm.com/docs/internals/drawdag
   */
  async function drawdag(dag: string): Promise<void> {
    // by default, drawdag sets date to 0,
    // but this would hide all commits in the ISL,
    // so we set "now" to our javascript date so all new commits are fetched,
    // then make our commits relative to that
    const pythonLabel = 'python:';
    const input = `${dag}
${dag.includes(pythonLabel) ? '' : pythonLabel}
now('${new Date().toISOString()}')  # set the "now" time
commit(date='now')
    `;
    await sl(['debugdrawdag'], {input});
  }

  // set up empty repo
  await sl(['init', '--config=format.use-eager-repo=True', '--config=init.prefer-git=False', '.']);
  await writeFileInRepo('.watchmanconfig', '{}');
  // write to repo config
  await writeFileInRepo(
    '.sl/config',
    ([['paths', [`default=eager:${repoDir}`]]] as [string, string[]][])
      .map(([section, configs]) => `[${section}]\n${configs.join('\n')}`)
      .join('\n'),
  );
  await writeFileInRepo('file.txt', 'hello');
  await sl(['commit', '-A', '-m', 'Initial Commit']);

  const {
    serverToClient,
    clientToServer,
    dispose: disposeClientConnection,
  } = mockedClientMessagebus as unknown as MockedClientMessageBus;

  // start "server" in the same process, connected to fake client message bus via eventEmitters
  const disposeServer = onClientConnection({
    cwd: repoDir,
    version: 'integration-test',
    command: 'sl',
    logger: mockLogger,

    postMessage(message: string): Promise<boolean> {
      serverToClient.emit('data', message);
      return Promise.resolve(true);
    },
    onDidReceiveMessage(handler: (event: Buffer, isBinary: boolean) => void | Promise<void>): {
      dispose(): void;
    } {
      const cb = (e: string | ArrayBuffer) => {
        e instanceof ArrayBuffer
          ? handler(e as Buffer, true)
          : handler(Buffer.from(e, 'utf8'), false);
      };
      clientToServer.on('data', cb);
      return {
        dispose: () => {
          clientToServer.off('data', cb);
        },
      };
    },
  });

  // render the entire app, which automatically starts the connection to the server
  render(<App />);

  return {
    repoDir,
    sl,
    cleanup: async () => {
      disposeServer();
      disposeClientConnection();
      // rm -rf the temp dir with the repo in it
      await retry(() => fs.promises.rm(repoDir, {recursive: true, force: true}));
    },
    writeFileInRepo,
    drawdag,
  };
}

async function retry<T>(cb: () => Promise<T>): Promise<T> {
  try {
    return await cb();
  } catch {
    return cb();
  }
}
