/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as fs from 'node:fs';

/** Matches the exit code Node itself uses for a process killed by an uncaught exception. */
export const UNCAUGHT_EXCEPTION_EXIT_CODE = 1;

/**
 * Record `err` in the server log, then terminate the process.
 *
 * Node makes no guarantee about the state of a process that continues past an uncaught exception,
 * so a server that keeps answering source control requests in that state is worse than one that
 * dies: the next `sl web` starts a fresh server, and open ISL clients already reconnect on their
 * own.
 *
 * The append is awaited so the record actually lands before we exit, and its failure is swallowed.
 * A rejection escaping here would be delivered straight back to this same handler — Node's default
 * is `--unhandled-rejections=throw` — and its own failing append would re-enter it without bound.
 */
export async function logUncaughtExceptionAndExit(
  err: Error | undefined,
  logFileLocation: string,
  exit: (code: number) => void = code => process.exit(code),
): Promise<void> {
  try {
    await fs.promises.appendFile(
      logFileLocation,
      `\n[${new Date().toString()}] ISL server child process got an uncaught exception:\n${
        err?.stack ?? err?.message
      }\n\n`,
      'utf8',
    );
  } catch {
    // Nowhere left to report this: the forked server is spawned with stdio: 'ignore'.
  }
  exit(UNCAUGHT_EXCEPTION_EXIT_CODE);
}

/** Install the process-wide `uncaughtException` handler for the forked ISL server process. */
export function registerUncaughtExceptionHandler(logFileLocation: string): void {
  process.on('uncaughtException', (err: Error) =>
    // The handler itself must never leave a rejection behind, or Node hands it right back to us.
    logUncaughtExceptionAndExit(err, logFileLocation).catch(() => undefined),
  );
}
