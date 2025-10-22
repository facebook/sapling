/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ChildProcess, IOType, Serializable, SpawnOptions} from 'node:child_process';
import type {Stream} from 'node:stream';

import getStream from 'get-stream';
import {spawn} from 'node:child_process';
import {Readable} from 'node:stream';
import os from 'os';
import {truncate} from './utils';

const LF = '\n';
const LF_BINARY = LF.codePointAt(0);
const CR = '\r';
const CR_BINARY = CR.codePointAt(0);

function maybeStripFinalNewline<T extends string | Uint8Array>(input: T, strip: boolean): T {
  if (!strip) {
    return input;
  }
  const isString = typeof input === 'string';
  const LF = isString ? '\n' : '\n'.codePointAt(0);
  const CR = isString ? '\r' : '\r'.codePointAt(0);
  if (typeof input === 'string') {
    const stripped = input.at(-1) === LF ? input.slice(0, input.at(-2) === CR ? -2 : -1) : input;
    return stripped as T;
  }

  const stripped =
    input.at(-1) === LF_BINARY ? input.subarray(0, input.at(-2) === CR_BINARY ? -2 : -1) : input;

  return stripped as T;
}

export interface EjecaOptions {
  /**
   * Current working directory of the child process.
   * @default process.cwd()
   */
  readonly cwd?: string;

  /**
   * Environment key-value pairs. Extends automatically if `process.extendEnv` is set to true.
   * @default process.env
   */
  readonly env?: NodeJS.ProcessEnv;

  /**
   * Set to `false` if you don't want to extend the environment variables when providing the `env` property.
   * @default true
   */
  readonly extendEnv?: boolean;

  /**
   * Feeds its contents as the standard input of the binary.
   */
  readonly input?: string | Buffer | ReadableStream;

  /**
   * Setting this to `false` resolves the promise with the error instead of rejecting it.
   * @default true
   */
  readonly reject?: boolean;

  /**
   * Same options as [`stdio`](https://nodejs.org/docs/latest-v18.x/api/child_process.html#optionsstdio).
   * @default 'pipe'
   */
  readonly stdin?: IOType | Stream | number | null | undefined;

  /**
   * Same options as [`stdio`](https://nodejs.org/docs/latest-v18.x/api/child_process.html#optionsstdio).
   * @default 'pipe'
   */
  readonly stdout?: IOType | Stream | number | null | undefined;

  /**
   * Same options as [`stdio`](https://nodejs.org/docs/latest-v18.x/api/child_process.html#optionsstdio).
   * @default 'pipe'
   */
  readonly stderr?: IOType | Stream | number | null | undefined;

  /**
   * Strip the final newline character from the (awaitable) output.
   * @default true
   */
  readonly stripFinalNewline?: boolean;

  /**
   * Whether a NodeIPC channel should be open. See the docs about [`stdio`](https://nodejs.org/docs/latest-v18.x/api/child_process.html#optionsstdio) for more info.
   * @default false
   */
  readonly ipc?: boolean;
}

interface KillOptions {
  /**
   * Milliseconds to wait for the child process to terminate before sending `SIGKILL`.
   * Can be disabled with `false`.
   * @default 5000
   */
  forceKillAfterTimeout?: number | boolean;
}

type KillParam = number | NodeJS.Signals | undefined;
const DEFAULT_FORCE_KILL_TIMEOUT = 1000 * 5;

function spawnedKill(
  kill: ChildProcess['kill'],
  signal: KillParam = 'SIGTERM',
  options: KillOptions = {},
): boolean {
  const killResult = kill(signal);

  if (shouldForceKill(signal, options, killResult)) {
    const timeout = getForceKillAfterTimeout(options);
    setTimeout(() => {
      kill('SIGKILL');
    }, timeout);
  }

  return killResult;
}

function getForceKillAfterTimeout({forceKillAfterTimeout = true}: KillOptions): number {
  if (typeof forceKillAfterTimeout !== 'number') {
    return DEFAULT_FORCE_KILL_TIMEOUT;
  }

  if (!Number.isFinite(forceKillAfterTimeout) || forceKillAfterTimeout < 0) {
    throw new TypeError(
      `Expected the \`forceKillAfterTimeout\` option to be a non-negative integer, got \`${forceKillAfterTimeout}\` (${typeof forceKillAfterTimeout})`,
    );
  }

  return forceKillAfterTimeout;
}

function shouldForceKill(
  signal: KillParam,
  {forceKillAfterTimeout}: KillOptions,
  killResult: boolean,
): boolean {
  const isSigTerm = signal === os.constants.signals.SIGTERM || signal == 'SIGTERM';
  return isSigTerm && forceKillAfterTimeout !== false && killResult;
}

export interface EjecaReturn {
  /**
   * The exit code if the child exited on its own.
   */
  exitCode: number;

  /**
   * The signal by which the child process was terminated, `undefined` if the process was not killed.
   *
   * Essentially obtained through `signal` on the `exit` event from [`ChildProcess`](https://nodejs.org/docs/latest-v18.x/api/child_process.html#event-exit)
   */
  signal?: string;

  /**
   * The file and arguments that were run, escaped. Useful for logging.
   */
  escapedCommand: string;

  /**
   * The output of the process on stdout.
   */
  stdout: string;

  /**
   * The output of the process on stderr.
   */
  stderr: string;

  /**
   * Whether the process was killed.
   */
  killed: boolean;
}

interface EjecaChildPromise {
  catch<ResultType = never>(
    onRejected?: (reason: EjecaError) => ResultType | PromiseLike<ResultType>,
  ): Promise<EjecaReturn | ResultType>;

  /**
   * Essentially the same as [`subprocess.kill`](https://nodejs.org/docs/latest-v18.x/api/child_process.html#subprocesskillsignal), but
   * with the caveat of having the processes SIGKILL'ed after a few seconds if the original signal
   * didn't successfully terminate the process. This behavior is configurable through the `options` option.
   */
  kill(signal?: KillParam, options?: KillOptions): boolean;

  getOneMessage(): Promise<Serializable>;
}

export type EjecaChildProcess = ChildProcess & EjecaChildPromise & Promise<EjecaReturn>;

// The return value is a mixin of `childProcess` and `Promise`
function getMergePromise(
  spawned: ChildProcess,
  promise: Promise<EjecaReturn>,
): ChildProcess & Promise<EjecaReturn> {
  const s2 = Object.create(spawned);
  // @ts-expect-error: we are doing some good old monkey patching here
  s2.then = (...args) => {
    return promise.then(...args);
  };
  // @ts-expect-error: we are doing some good old monkey patching here
  s2.catch = (...args) => {
    return promise.catch(...args);
  };
  // @ts-expect-error: we are doing some good old monkey patching here
  s2.finally = (...args) => {
    return promise.finally(...args);
  };

  return s2 as unknown as ChildProcess & Promise<EjecaReturn>;
}

function escapedCmd(file: string, args: readonly string[]): string {
  const allargs = [file, ...args.map(arg => `"${arg.replace(/"/g, '\\"')}"`)];
  return allargs.join(' ');
}

// Use promises instead of `child_process` events
function getSpawnedPromise(
  spawned: ChildProcess,
  escapedCommand: string,
  options?: EjecaOptions,
): Promise<EjecaReturn> {
  const {stdout, stderr} = spawned;
  const spawnedPromise = new Promise<{exitCode: number; signal?: string}>((resolve, reject) => {
    spawned.on('exit', (exitCode, signal) => {
      resolve({exitCode: exitCode ?? -1, signal: signal ?? undefined});
    });

    spawned.on('error', error => {
      reject(error);
    });

    if (spawned.stdin) {
      spawned.stdin.on('error', error => {
        reject(error);
      });
    }
  });

  return Promise.all([spawnedPromise, getStreamPromise(stdout), getStreamPromise(stderr)]).then(
    values => {
      const [{exitCode, signal}, stdout, stderr] = values;
      const stripfinalNl = options?.stripFinalNewline ?? true;
      const ret: EjecaReturn = {
        exitCode,
        signal,
        stdout: maybeStripFinalNewline(stdout, stripfinalNl),
        stderr: maybeStripFinalNewline(stderr, stripfinalNl),
        killed: false,
        escapedCommand,
      };
      if (exitCode !== 0 || signal != undefined) {
        throw new EjecaError(ret);
      }
      return ret;
    },
  );
}

export class EjecaError extends Error implements EjecaReturn {
  escapedCommand: string;
  exitCode: number;
  signal?: string;
  stdout: string;
  stderr: string;
  killed: boolean;

  constructor(info: EjecaReturn) {
    const message =
      `Command \`${truncate(info.escapedCommand, 50)}\` ` +
      (info.signal != null ? 'was killed' : 'exited with non-zero status') +
      (info.signal != null ? ` with signal ${info.signal}` : ` with exit code ${info.exitCode}`);
    super(message);

    this.exitCode = info.exitCode;
    this.signal = info.signal;
    this.stdout = info.stdout;
    this.stderr = info.stderr;
    this.killed = info.killed;
    this.escapedCommand = info.escapedCommand;
  }

  toString() {
    return `${this.message}\n${JSON.stringify(this, undefined, 2)}\n`;
  }
}

function getStreamPromise(origStream: Stream | null): Promise<string> {
  const stream = origStream ?? new Readable({read() {}});
  return getStream(stream, {encoding: 'utf8'});
}

function commonToSpawnOptions(options?: EjecaOptions): SpawnOptions {
  const env = options?.env
    ? (options.extendEnv ?? true)
      ? {...process.env, ...options.env}
      : options.env
    : process.env;
  const stdin = options?.stdin ?? 'pipe';
  const stdout = options?.stdout ?? 'pipe';
  const stderr = options?.stderr ?? 'pipe';
  return {
    cwd: options?.cwd || process.cwd(),
    env,
    stdio: options?.ipc ? [stdin, stdout, stderr, 'ipc'] : [stdin, stdout, stderr],
    windowsHide: true,
  };
}

/**
 * Essentially a wrapper for [`child_process.spawn`](https://nodejs.org/docs/latest-v18.x/api/child_process.html#child_processspawncommand-args-options), which
 * additionally makes the result awaitable through `EjecaChildPromise`. `_file`, `_args` and `_options`
 * are essentially the same as the args for `child_process.spawn`.
 *
 * It also has a couple of additional features:
 * - Adds a forced timeout kill for `child_process.kill` through `EjecaChildPromise.kill`
 * - Allows feeding to stdin through `_options.input`
 */
export function ejeca(
  file: string,
  args: readonly string[],
  options?: EjecaOptions,
): EjecaChildProcess {
  const spawned = spawn(file, args, commonToSpawnOptions(options));
  const spawnedPromise = getSpawnedPromise(spawned, escapedCmd(file, args), options);
  const mergedPromise = getMergePromise(spawned, spawnedPromise);

  // TODO: Handle streams
  if (options && options.input) {
    mergedPromise.stdin?.end(options.input);
  }

  const ecp = Object.create(mergedPromise);
  ecp.kill = (p: KillParam, o?: KillOptions) => {
    return spawnedKill(s => mergedPromise.kill(s), p, o);
  };

  if (options && options.ipc) {
    ecp._ipcMessagesQueue = [];
    ecp._ipcPendingPromises = [];
    mergedPromise.on('message', message => {
      if (ecp._ipcPendingPromises.length > 0) {
        const resolve = ecp._ipcPendingPromises.shift()[0];
        resolve(message);
      } else {
        ecp._ipcMessagesQueue.push(message);
      }
    });
    mergedPromise.on('error', error => {
      while (ecp._ipcPendingPromises.length > 0) {
        const reject = ecp._ipcPendingPromises.shift()[1];
        reject(error);
      }
    });
    mergedPromise.on('exit', (_exitCode, _signal) => {
      while (ecp._ipcPendingPromises.length > 0) {
        const reject = ecp._ipcPendingPromises.shift()[1];
        reject(new Error('IPC channel closed before receiving a message'));
      }
    });

    ecp.getOneMessage = () => {
      return new Promise<string>((resolve, reject) => {
        if (ecp._ipcMessagesQueue.length > 0) {
          resolve(ecp._ipcMessagesQueue.shift());
        } else {
          ecp._ipcPendingPromises.push([resolve, reject]);
        }
      });
    };
  } else {
    ecp.getOneMessage = () => {
      throw new Error('IPC not enabled');
    };
  }

  return ecp as unknown as EjecaChildProcess;
}

/**
 * Extract the actually useful stderr part of the Ejeca Error, to avoid the long command args being printed first.
 */
export function simplifyEjecaError(error: EjecaError): Error {
  return new Error(error.stderr.trim() || error.message);
}
