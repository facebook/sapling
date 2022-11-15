/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import fs from 'fs';
import util from 'util';

export interface Logger {
  info(...args: Parameters<typeof console.info>): void;
  log(...args: Parameters<typeof console.log>): void;
  warn(...args: Parameters<typeof console.warn>): void;
  error(...args: Parameters<typeof console.error>): void;
}

export const stdoutLogger = console;

export function fileLogger(filename: string): Logger {
  const log = (...args: Parameters<typeof console.log>) => {
    const str = util.format(...args) + '\n';
    fs.promises.appendFile(filename, str);
  };

  return {
    info: log,
    log,
    warn: log,
    error: log,
  };
}
