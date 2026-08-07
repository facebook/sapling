/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {
  UNCAUGHT_EXCEPTION_EXIT_CODE,
  logUncaughtExceptionAndExit,
  registerUncaughtExceptionHandler,
} from '../uncaughtException';

describe('uncaught exception handler', () => {
  let tmp: string;
  let logFile: string;

  beforeEach(async () => {
    tmp = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'isl-uncaught-exception-test'));
    logFile = path.join(tmp, 'isl-server.log');
  });

  afterEach(async () => {
    jest.restoreAllMocks();
    await fs.promises.rm(tmp, {recursive: true, force: true});
  });

  it('records the exception and exits non-zero', async () => {
    const exit = jest.fn();

    await logUncaughtExceptionAndExit(new Error('kaboom'), logFile, exit);

    const contents = await fs.promises.readFile(logFile, 'utf8');
    expect(contents).toContain('ISL server child process got an uncaught exception');
    expect(contents).toContain('kaboom');
    expect(exit).toHaveBeenCalledTimes(1);
    expect(exit).toHaveBeenCalledWith(UNCAUGHT_EXCEPTION_EXIT_CODE);
    expect(UNCAUGHT_EXCEPTION_EXIT_CODE).not.toEqual(0);
  });

  it('does not truncate what the server already logged', async () => {
    await fs.promises.writeFile(logFile, 'earlier log line\n', 'utf8');

    await logUncaughtExceptionAndExit(new Error('kaboom'), logFile, jest.fn());

    expect(await fs.promises.readFile(logFile, 'utf8')).toContain('earlier log line');
  });

  it('still exits when the exception cannot be recorded', async () => {
    // An aggressive /tmp sweeper deleting the per-spawn mkdtemp log directory is the case that
    // originally livelocked this handler.
    await fs.promises.rm(tmp, {recursive: true});
    await expect(fs.promises.appendFile(logFile, 'precondition', 'utf8')).rejects.toMatchObject({
      code: 'ENOENT',
    });
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
      const exit = jest
        .spyOn(process, 'exit')
        .mockImplementation((() => undefined) as unknown as typeof process.exit);

      await register(logFile)(new Error('kaboom'));

      expect(await fs.promises.readFile(logFile, 'utf8')).toContain('kaboom');
      expect(exit).toHaveBeenCalledWith(UNCAUGHT_EXCEPTION_EXIT_CODE);
    });

    it('settles rather than rejecting when the log write fails', async () => {
      jest
        .spyOn(process, 'exit')
        .mockImplementation((() => undefined) as unknown as typeof process.exit);
      await fs.promises.rm(tmp, {recursive: true});
      const handler = register(logFile);

      // A rejection here is what Node feeds back into this very handler, which is how the
      // original livelock sustained itself. Jest fails the suite on any unhandled rejection,
      // so a fire-and-forget regression goes red even though it cannot be asserted in-process:
      // jest-environment-node hands tests a copy of `process` that Node never emits on.
      await expect(handler(new Error('kaboom'))).resolves.toBeUndefined();
    });
  });
});
