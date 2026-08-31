/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import fs from 'node:fs';
import {
  UNCAUGHT_EXCEPTION_EXIT_CODE,
  logUncaughtExceptionAndExit,
  registerUncaughtExceptionHandler,
} from '../uncaughtException';

describe('uncaught exception handler', () => {
  const logFile = 'isl-server.log';

  afterEach(() => {
    jest.restoreAllMocks();
  });

  it('records the exception and exits non-zero', async () => {
    const appendFile = jest.spyOn(fs.promises, 'appendFile').mockResolvedValue(undefined);
    const exit = jest.fn();

    await logUncaughtExceptionAndExit(new Error('kaboom'), logFile, exit);

    expect(appendFile).toHaveBeenCalledWith(
      logFile,
      expect.stringMatching(/ISL server child process got an uncaught exception:[\s\S]*kaboom/),
      'utf8',
    );
    expect(exit).toHaveBeenCalledTimes(1);
    expect(exit).toHaveBeenCalledWith(UNCAUGHT_EXCEPTION_EXIT_CODE);
    expect(UNCAUGHT_EXCEPTION_EXIT_CODE).not.toEqual(0);
  });

  it('appends rather than truncating what the server already logged', async () => {
    const appendFile = jest.spyOn(fs.promises, 'appendFile').mockResolvedValue(undefined);
    const writeFile = jest.spyOn(fs.promises, 'writeFile');

    await logUncaughtExceptionAndExit(new Error('kaboom'), logFile, jest.fn());

    expect(appendFile).toHaveBeenCalledTimes(1);
    expect(writeFile).not.toHaveBeenCalled();
  });

  it('still exits when the exception cannot be recorded', async () => {
    jest
      .spyOn(fs.promises, 'appendFile')
      .mockRejectedValue(Object.assign(new Error('no such file or directory'), {code: 'ENOENT'}));
    const exit = jest.fn();

    await expect(
      logUncaughtExceptionAndExit(new Error('kaboom'), logFile, exit),
    ).resolves.toBeUndefined();

    expect(exit).toHaveBeenCalledTimes(1);
    expect(exit).toHaveBeenCalledWith(UNCAUGHT_EXCEPTION_EXIT_CODE);
  });

  describe('registerUncaughtExceptionHandler', () => {
    let registered: Array<(err: Error) => Promise<void>> = [];

    /** Registers the production handler and returns the listener Node would invoke. */
    function register(location: string): (err: Error) => Promise<void> {
      const before = process.listeners('uncaughtException').length;
      registerUncaughtExceptionHandler(location);
      const listeners = process.listeners('uncaughtException');
      expect(listeners).toHaveLength(before + 1);
      const listener = listeners[listeners.length - 1] as unknown as (err: Error) => Promise<void>;
      registered.push(listener);
      return listener;
    }

    afterEach(() => {
      for (const listener of registered) {
        process.off('uncaughtException', listener as unknown as NodeJS.UncaughtExceptionListener);
      }
      registered = [];
    });

    it('records the exception and exits the process', async () => {
      const appendFile = jest.spyOn(fs.promises, 'appendFile').mockResolvedValue(undefined);
      const exit = jest
        .spyOn(process, 'exit')
        .mockImplementation((() => undefined) as unknown as typeof process.exit);

      await register(logFile)(new Error('kaboom'));

      expect(appendFile).toHaveBeenCalledWith(logFile, expect.stringContaining('kaboom'), 'utf8');
      expect(exit).toHaveBeenCalledWith(UNCAUGHT_EXCEPTION_EXIT_CODE);
    });

    it('settles rather than rejecting when the log write fails', async () => {
      jest.spyOn(fs.promises, 'appendFile').mockRejectedValue(new Error('write failed'));
      jest
        .spyOn(process, 'exit')
        .mockImplementation((() => undefined) as unknown as typeof process.exit);
      const handler = register(logFile);

      // A rejection here is what Node feeds back into this very handler, which is how the
      // original livelock sustained itself. Jest fails the suite on any unhandled rejection,
      // so a fire-and-forget regression goes red even though it cannot be asserted in-process:
      // jest-environment-node hands tests a copy of `process` that Node never emits on.
      await expect(handler(new Error('kaboom'))).resolves.toBeUndefined();
    });
  });
});
