/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Level} from './logger';

import fs from 'node:fs';
import path from 'node:path';
import util from 'node:util';
import {Logger} from './logger';

/** Logger that outputs to a given filename.
 * Typically used for browser ISL's server. */
export class FileLogger extends Logger {
  /**
   * Tail of the chain of appends issued so far. Never rejects: `write` is fire-and-forget,
   * and an unhandled rejection here would terminate the server process.
   */
  private pendingWrites: Promise<void> = Promise.resolve();
  /** Whether we already reported that appending to the log file is failing. */
  private reportedFailure = false;

  constructor(public filename: string) {
    super();
  }

  write(level: Level, timeStr: string, ...args: Parameters<typeof console.log>): void {
    const str = util.format(timeStr, this.levelToString(level), ...args) + '\n';
    this.pendingWrites = this.pendingWrites
      .then(() => this.append(str))
      .catch(error => this.reportFailure(error));
  }

  /** Resolves once every write issued so far has been flushed or given up on. Tests only. */
  flushForTests(): Promise<void> {
    return this.pendingWrites;
  }

  private async append(str: string): Promise<void> {
    try {
      await fs.promises.appendFile(this.filename, str);
    } catch (error) {
      // The log file usually lives in a temp dir, which an aggressive /tmp cleaner may delete
      // out from under a long-running server. Recreate it and retry once so that logging
      // recovers instead of going dark for the rest of the session.
      // 0o700 mirrors the mkdtemp posture in proxy/startServer.ts: /tmp is world-writable and
      // the log carries sl argv and repo paths, so the default 0o755 would widen access.
      if ((error as NodeJS.ErrnoException).code === 'ENOENT') {
        try {
          await fs.promises.mkdir(path.dirname(this.filename), {recursive: true, mode: 0o700});
          await fs.promises.appendFile(this.filename, str);
          return;
        } catch (retryError) {
          this.reportFailure(retryError);
          return;
        }
      }
      this.reportFailure(error);
    }
  }

  /** Announce, at most once per logger, that log lines are being dropped. */
  private reportFailure(error: unknown): void {
    if (this.reportedFailure) {
      return;
    }
    this.reportedFailure = true;
    // eslint-disable-next-line no-console
    console.error(`failed to write to log file ${this.filename}, some logs will be lost:`, error);
  }

  getLogFileContents() {
    return fs.promises.readFile(this.filename, 'utf-8');
  }
}
