/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {FileLogger} from '../FileLogger';

describe('FileLogger', () => {
  let tempDir: string;
  let logDir: string;
  let logFile: string;

  beforeEach(async () => {
    tempDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'isl-file-logger-test-'));
    logDir = path.join(tempDir, 'isl-server-log');
    logFile = path.join(logDir, 'isl-server.log');
    await fs.promises.mkdir(logDir);
  });

  afterEach(async () => {
    jest.restoreAllMocks();
    await fs.promises.rm(tempDir, {recursive: true, force: true});
  });

  it('appends log lines to the file', async () => {
    const appendFile = jest.spyOn(fs.promises, 'appendFile').mockResolvedValue(undefined);
    const logger = new FileLogger(logFile);
    logger.write('info', '[time]', 'hello');
    logger.write('error', '[time]', 'goodbye');
    await logger.flushForTests();

    expect(appendFile).toHaveBeenNthCalledWith(1, logFile, '[time]  [INFO] hello\n');
    expect(appendFile).toHaveBeenNthCalledWith(2, logFile, '[time] [ERROR] goodbye\n');
  });

  it('does not reject when the log directory has been deleted', async () => {
    const logger = new FileLogger(logFile);
    await fs.promises.rm(logDir, {recursive: true});

    logger.info('the directory disappeared');
    await expect(logger.flushForTests()).resolves.toBeUndefined();
  });

  it('recreates the deleted log directory and writes the line', async () => {
    const logger = new FileLogger(logFile);
    await fs.promises.rm(logDir, {recursive: true});

    logger.info('the directory disappeared');
    await logger.flushForTests();

    const contents = await fs.promises.readFile(logFile, 'utf-8');
    expect(contents).toContain(' [INFO] the directory disappeared\n');
  });

  it('retries at most once per line and reports failure only once', async () => {
    const consoleError = jest.spyOn(console, 'error').mockImplementation(() => undefined);
    const appendFile = jest
      .spyOn(fs.promises, 'appendFile')
      .mockRejectedValue(Object.assign(new Error('no such file or directory'), {code: 'ENOENT'}));
    const mkdir = jest.spyOn(fs.promises, 'mkdir').mockResolvedValue(undefined);

    const logger = new FileLogger(logFile);
    logger.info('one');
    logger.info('two');
    await expect(logger.flushForTests()).resolves.toBeUndefined();

    // two lines, each attempted once and then retried once after recreating the directory
    expect(appendFile).toHaveBeenCalledTimes(4);
    expect(mkdir).toHaveBeenCalledTimes(2);
    expect(consoleError).toHaveBeenCalledTimes(1);
  });

  it('does not retry errors other than ENOENT', async () => {
    const consoleError = jest.spyOn(console, 'error').mockImplementation(() => undefined);
    const appendFile = jest
      .spyOn(fs.promises, 'appendFile')
      .mockRejectedValue(Object.assign(new Error('permission denied'), {code: 'EACCES'}));
    const mkdir = jest.spyOn(fs.promises, 'mkdir');

    const logger = new FileLogger(logFile);
    logger.info('one');
    await expect(logger.flushForTests()).resolves.toBeUndefined();

    expect(appendFile).toHaveBeenCalledTimes(1);
    expect(mkdir).not.toHaveBeenCalled();
    expect(consoleError).toHaveBeenCalledTimes(1);
  });
});
