/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {readExistingServerFile} from '../existingServerStateFiles';
import * as startServer from '../server';
import * as lifecycle from '../serverLifecycle';
import {runProxyMain, parseArgs} from '../startServer';
import * as util from 'util';

/* eslint-disable require-await */

// to prevent permission issues and races, mock FS read/writes in memory.
let mockFsData: {[key: string]: string} = {};
jest.mock('fs', () => {
  return {
    promises: {
      writeFile: jest.fn(async (path: string, data: string) => {
        mockFsData[path] = data;
      }),
      readFile: jest.fn(async (path: string) => {
        return mockFsData[path];
      }),
      stat: jest.fn(async (_path: string) => {
        return {mode: 0o700, isSymbolicLink: () => false};
      }),
      rm: jest.fn(async (path: string) => {
        delete mockFsData[path];
      }),
      mkdir: jest.fn(async (_path: string) => {
        //
      }),
      mkdtemp: jest.fn(async (_path: string) => {
        return '/tmp/';
      }),
    },
  };
});

describe('run-proxy', () => {
  let stdout: Array<string> = [];
  let stderr: Array<string> = [];
  function allConsoleStdout() {
    return stdout.join('\n');
  }
  function resetStdout() {
    stdout = [];
    stderr = [];
  }
  beforeEach(() => {
    resetStdout();
    const appendStdout = jest.fn((...args) => stdout.push(util.format(...args)));
    const appendStderr = jest.fn((...args) => stderr.push(util.format(...args)));

    global.console = {
      log: appendStdout,
      info: appendStdout,
      warn: appendStdout,
      error: appendStderr,
    } as unknown as Console;

    // reset mock filesystem
    mockFsData = {};

    jest.clearAllMocks();
  });

  const killMock = jest.spyOn(process, 'kill').mockImplementation(() => true);
  const exitMock = jest.spyOn(process, 'exit').mockImplementation((): never => {
    throw new Error('exited');
  });

  const defaultArgs = {
    help: false,
    // subprocess spawning without --foreground doesn't work well in tests
    // plus we don't want to manage closing servers after the tests
    foreground: true,
    // we don't want to actually open the url in the browser during a test
    openUrl: false,
    port: 3011,
    isDevMode: false,
    json: false,
    stdout: false,
    platform: undefined,
    kill: false,
    force: false,
    slVersion: '1.0',
    command: 'sl',
  };

  it('spawns a server', async () => {
    const startServerSpy = jest
      .spyOn(startServer, 'startServer')
      .mockImplementation(() => Promise.resolve({type: 'success', port: 3011, pid: 1000}));

    await runProxyMain(defaultArgs);

    expect(startServerSpy).toHaveBeenCalledTimes(1);
  });

  it('can output json', async () => {
    jest
      .spyOn(startServer, 'startServer')
      .mockImplementation(() => Promise.resolve({type: 'success', port: 3011, pid: 1000}));

    await runProxyMain({...defaultArgs, json: true});

    expect(JSON.parse(allConsoleStdout())).toEqual(
      expect.objectContaining({
        command: 'sl',
        cwd: expect.stringContaining('isl-server'),
        logFileLocation: expect.stringContaining('isl-server.log'),
        pid: 1000,
        port: 3011,
        token: expect.stringMatching(/[a-z0-9]{32}/),
        url: expect.stringContaining('http://localhost:3011/'),
        wasServerReused: false,
      }),
    );
  });

  it('writes existing server info', async () => {
    jest
      .spyOn(startServer, 'startServer')
      .mockImplementation(() => Promise.resolve({type: 'success', port: 3011, pid: 1000}));

    await expect(readExistingServerFile(3011)).rejects.toEqual(expect.anything());

    await runProxyMain(defaultArgs);

    expect(await readExistingServerFile(3011)).toEqual(
      expect.objectContaining({
        sensitiveToken: expect.anything(),
        challengeToken: expect.anything(),
        command: 'sl',
        slVersion: '1.0',
      }),
    );
  });

  it('can output json for a re-used server', async () => {
    jest
      .spyOn(startServer, 'startServer')
      .mockImplementationOnce(() => Promise.resolve({type: 'success', port: 3011, pid: 1000}))
      .mockImplementationOnce(() => Promise.resolve({type: 'addressInUse'}));

    jest.spyOn(lifecycle, 'checkIfServerIsAliveAndIsISL').mockImplementation(() => {
      return Promise.resolve(1000);
    });

    await runProxyMain(defaultArgs);
    resetStdout();

    await expect(() => runProxyMain({...defaultArgs, json: true})).rejects.toEqual(
      new Error('exited'),
    );

    expect(JSON.parse(allConsoleStdout())).toEqual(
      expect.objectContaining({
        command: 'sl',
        cwd: expect.stringContaining('isl-server'),
        logFileLocation: expect.stringContaining('isl-server.log'),
        pid: 1000,
        port: 3011,
        token: expect.stringMatching(/[a-z0-9]{32}/),
        url: expect.stringContaining('http://localhost:3011/'),
        wasServerReused: true,
      }),
    );
    expect(exitMock).toHaveBeenCalledWith(0);
  });

  it('can kill a server', async () => {
    const startServerSpy = jest
      .spyOn(startServer, 'startServer')
      .mockImplementation(() => Promise.resolve({type: 'success', port: 3011, pid: 1000}));

    jest.spyOn(lifecycle, 'checkIfServerIsAliveAndIsISL').mockImplementation(() => {
      return Promise.resolve(1000);
    });

    // successfully start normally
    await runProxyMain(defaultArgs);

    // now run with --kill
    await expect(() => runProxyMain({...defaultArgs, kill: true})).rejects.toEqual(
      new Error('exited'),
    );

    expect(killMock).toHaveBeenCalled();
    expect(exitMock).toHaveBeenCalledWith(0); // exits after killing
    expect(startServerSpy).toHaveBeenCalledTimes(1); // called for original server only
  });

  it('--force kills and starts a new server', async () => {
    const startServerSpy = jest
      .spyOn(startServer, 'startServer')
      .mockImplementation(() => Promise.resolve({type: 'success', port: 3011, pid: 1000}));

    jest.spyOn(lifecycle, 'checkIfServerIsAliveAndIsISL').mockImplementation(() => {
      return Promise.resolve(1000);
    });

    // successfully start normally
    await runProxyMain(defaultArgs);

    // now run with --force
    await runProxyMain({...defaultArgs, force: true});

    expect(killMock).toHaveBeenCalled();
    expect(exitMock).not.toHaveBeenCalled();
    expect(startServerSpy).toHaveBeenCalledTimes(2); // original to be killed and new instance
  });

  it('forces a fresh server if sl version changed', async () => {
    jest
      .spyOn(startServer, 'startServer')
      .mockImplementationOnce(() => Promise.resolve({type: 'success', port: 3011, pid: 1000}))
      .mockImplementationOnce(() => Promise.resolve({type: 'addressInUse'}));

    jest.spyOn(lifecycle, 'checkIfServerIsAliveAndIsISL').mockImplementation(() => {
      return Promise.resolve(1000);
    });

    await runProxyMain({...defaultArgs, slVersion: '0.1'});
    resetStdout();

    await runProxyMain({...defaultArgs, json: true, slVersion: '0.2'});

    expect(JSON.parse(allConsoleStdout())).toEqual(
      expect.objectContaining({
        wasServerReused: false,
      }),
    );
  });

  it('forces a fresh server if sl command changed', async () => {
    jest
      .spyOn(startServer, 'startServer')
      .mockImplementationOnce(() => Promise.resolve({type: 'success', port: 3011, pid: 1000}))
      .mockImplementationOnce(() => Promise.resolve({type: 'addressInUse'}));

    jest.spyOn(lifecycle, 'checkIfServerIsAliveAndIsISL').mockImplementation(() => {
      return Promise.resolve(1000);
    });

    await runProxyMain({...defaultArgs, command: 'sl'});
    resetStdout();

    await runProxyMain({...defaultArgs, json: true, command: '/bin/sl'});

    expect(JSON.parse(allConsoleStdout())).toEqual(
      expect.objectContaining({
        wasServerReused: false,
      }),
    );
  });
});

describe('argument parsing', () => {
  it('can parse arguments', () => {
    expect(parseArgs(['--port', '3001', '--force'])).toEqual(
      expect.objectContaining({
        port: 3001,
        force: true,
      }),
    );
  });
});
