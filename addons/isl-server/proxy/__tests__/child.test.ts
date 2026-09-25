/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {StartServerResult} from '../server';

jest.mock('../server', () => ({startServer: jest.fn()}));
jest.mock('../uncaughtException', () => ({registerUncaughtExceptionHandler: jest.fn()}));

describe('background server launcher connection', () => {
  const send = jest.fn();
  const result: StartServerResult = {type: 'success', port: 3001, pid: 123};
  const descriptors = Object.getOwnPropertyDescriptors(process);
  const serverArgs = process.env.ISL_SERVER_ARGS;

  beforeEach(() => {
    jest.resetModules();
    send.mockReset();
    Object.defineProperty(process, 'send', {configurable: true, value: send});
    Object.defineProperty(process, 'connected', {configurable: true, value: true});
    process.env.ISL_SERVER_ARGS = JSON.stringify({logFileLocation: 'isl-server.log'});
  });

  afterEach(() => {
    for (const key of ['send', 'connected'] as const) {
      if (descriptors[key]) {
        Object.defineProperty(process, key, descriptors[key]);
      } else {
        Reflect.deleteProperty(process, key);
      }
    }
    if (serverArgs == null) {
      delete process.env.ISL_SERVER_ARGS;
    } else {
      process.env.ISL_SERVER_ARGS = serverArgs;
    }
  });

  async function start() {
    const {startServer} = await import('../server');
    jest.mocked(startServer).mockResolvedValue(result);
    await import('../child');
    await new Promise(resolve => setImmediate(resolve));
    return jest.mocked(startServer).mock.calls[0][0].logInfo;
  }

  it('delivers startup results while the launcher is connected', async () => {
    await start();
    expect(send).toHaveBeenCalledWith(
      {type: 'result', result},
      undefined,
      {swallowErrors: true},
      expect.any(Function),
    );
  });

  it('can reject stale clients after the launcher exits', async () => {
    const logInfo = await start();
    send.mockClear();
    Object.defineProperty(process, 'connected', {value: false});

    logInfo('closing ws:', 'Invalid token');

    expect(send).not.toHaveBeenCalled();
  });

  it('handles the launcher disconnecting during a send', async () => {
    const logInfo = await start();
    send.mockImplementation((_message, _handle, _options, callback) => {
      callback(Object.assign(new Error('Channel closed'), {code: 'ERR_IPC_CHANNEL_CLOSED'}));
    });

    expect(() => logInfo('closing ws:', 'Invalid token')).not.toThrow();
  });
});
