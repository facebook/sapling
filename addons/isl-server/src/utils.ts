/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ExecaChildProcess} from 'execa';

import os from 'os';

export function sleep(timeMs: number): Promise<void> {
  return new Promise(res => setTimeout(res, timeMs));
}

export function firstOfIterable<T>(iterable: IterableIterator<T>): T | undefined {
  return iterable.next().value;
}

/**
 * Limits async function execution parallelism to only one at a time.
 * Hence, if a call is already running, it will wait for it to finish,
 * then start the next async execution, but if called again while not finished,
 * it will return the scheduled execution promise.
 *
 * Sample Usage:
 * ```
 * let i = 1;
 * const oneExecAtATime = serializeAsyncCall(() => {
 *   return new Promise((resolve, reject) => {
 *     setTimeout(200, () => resolve(i++));
 *   });
 * });
 *
 * const result1Promise = oneExecAtATime(); // Start an async, and resolve to 1 in 200 ms.
 * const result2Promise = oneExecAtATime(); // Schedule the next async, and resolve to 2 in 400 ms.
 * const result3Promise = oneExecAtATime(); // Reuse scheduled promise and resolve to 2 in 400 ms.
 * ```
 */
export function serializeAsyncCall<T>(asyncFun: () => Promise<T>): () => Promise<T> {
  let scheduledCall: Promise<T> | undefined = undefined;
  let pendingCall: Promise<undefined> | undefined = undefined;
  const startAsyncCall = () => {
    const resultPromise = asyncFun();
    pendingCall = resultPromise.then(
      () => (pendingCall = undefined),
      () => (pendingCall = undefined),
    );
    return resultPromise;
  };
  const callNext = () => {
    scheduledCall = undefined;
    return startAsyncCall();
  };
  const scheduleNextCall = () => {
    if (scheduledCall == null) {
      if (pendingCall == null) {
        throw new Error('pendingCall must not be null!');
      }
      scheduledCall = pendingCall.then(callNext, callNext);
    }
    return scheduledCall;
  };
  return () => {
    if (pendingCall == null) {
      return startAsyncCall();
    } else {
      return scheduleNextCall();
    }
  };
}

/**
 * Kill `child` on `AbortSignal`.
 *
 * This is slightly more robust than execa 6.0 and nodejs' `signal` support:
 * if a process was stopped (by `SIGTSTP` or `SIGSTOP`), it can still be killed.
 */
export function handleAbortSignalOnProcess(child: ExecaChildProcess, signal: AbortSignal) {
  signal.addEventListener('abort', () => {
    if (os.platform() == 'win32') {
      // Signals are ignored on Windows.
      // execa's default forceKillAfterTimeout behavior does not
      // make sense for Windows. Disable it explicitly.
      child.kill('SIGKILL', {forceKillAfterTimeout: false});
    } else {
      // If the process is stopped (ex. Ctrl+Z, kill -STOP), make it
      // continue first so it can respond to signals including SIGKILL.
      child.kill('SIGCONT');
      // A good citizen process should exit soon after recieving SIGTERM.
      // In case it doesn't, send SIGKILL after 5 seconds.
      child.kill('SIGTERM', {forceKillAfterTimeout: 5000});
    }
  });
}
