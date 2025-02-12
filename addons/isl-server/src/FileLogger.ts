/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Level} from './logger';

import fs from 'node:fs';
import util from 'node:util';
import {Logger} from './logger';

/** Logger that outputs to a given filename.
 * Typically used for browser ISL's server. */
export class FileLogger extends Logger {
  constructor(public filename: string) {
    super();
  }

  write(level: Level, timeStr: string, ...args: Parameters<typeof console.log>): void {
    const str = util.format(timeStr, this.levelToString(level), ...args) + '\n';
    void fs.promises.appendFile(this.filename, str);
  }

  getLogFileContents() {
    return fs.promises.readFile(this.filename, 'utf-8');
  }
}
