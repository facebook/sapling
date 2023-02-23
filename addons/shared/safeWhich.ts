/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import fs from 'fs';
import path from 'path';
import util from 'util';

/**
 * This is intended to be a workaround for NodeJs's failure to honor
 * NoDefaultCurrentDirectoryInExePath on Windows as reported here:
 *
 * - https://github.com/nodejs/node/issues/46264
 * - https://github.com/libuv/libuv/issues/3888
 *
 * This function is intended to be used to wrap the first argument passed to a
 * function like `child_process.spawn()` or `execa()` to ensure that if all
 * of the following are true:
 *
 * - The program is being run on Windows.
 * - The `NoDefaultCurrentDirectoryInExePath` environment variable is set.
 * - The argument is NOT an absolute path.
 *
 * Then this will take the argument and resolve it to an absolute path the way
 * that which(1) would on UNIX, using the PATH and PATHEXT environment variables
 * on{Windows, *excluding* the current directory when considering the PATH.
 */
export function safeWhich(exe: string): Promise<string> {
  if (canRunDirectly(exe)) {
    return Promise.resolve(exe);
  }

  const {PATH, PATHEXT} = process.env;
  return safeWhichUsingEnvironment({exe, PATH: PATH ?? '', PATHEXT: PATHEXT ?? ''});
}

/**
 * safeWhich() with PATH and PATHEXT environment variables passed in.
 */
export async function safeWhichUsingEnvironment({
  exe,
  PATH,
  PATHEXT,
}: {
  exe: string;
  PATH: string;
  PATHEXT: string;
}): Promise<string> {
  // Put empty string at the front of the "extensions" list so that
  // if the full name of the executable is passed in, it matches first.
  const extensions = [''].concat(PATHEXT.split(path.delimiter));
  for (const directory of PATH.split(path.delimiter)) {
    // Ignore the current directory because NoDefaultCurrentDirectoryInExePath is set.
    if (directory === '.') {
      continue;
    }

    for (const extension of extensions) {
      // TODO: What happens if `directory` itself contains an environment
      // variable, such as `%SystemRoot%\system32`?
      const absolutePath = path.resolve(directory, `${exe}${extension}`);
      try {
        // eslint-disable-next-line no-bitwise, no-await-in-loop
        await fs.promises.access(absolutePath, fs.constants.R_OK | fs.constants.X_OK);
        return absolutePath;
      } catch (_e) {
        // absolutePath is not a valid executable: try the next candidate.
      }
    }
  }

  throwFileNotFound(exe);
}

function canRunDirectly(exe: string): boolean {
  if (
    process.platform !== 'win32' ||
    // eslint-disable-next-line no-prototype-builtins
    !process.env.hasOwnProperty('NoDefaultCurrentDirectoryInExePath') ||
    path.isAbsolute(exe)
  ) {
    return true;
  }

  const {PATH} = process.env;
  if (PATH == null) {
    throwFileNotFound(exe);
  }

  return false;
}

/** See https://nodejs.org/api/errors.html#class-systemerror. */
interface SystemError extends Error {
  code: string;
  errno?: number;
}

function throwFileNotFound(file: string): never {
  const error = new Error(`file not found: ${file}`);
  const code = 'ENOENT';
  (error as SystemError).code = 'ENOENT';
  // In Node v14.17.0 / v16.0.0 or later, one could iterate the entries of
  // util.getSystemErrorMap() to determine the errno associated with ENOENT.
  if (typeof util.getSystemErrorMap === 'function') {
    for (const [errno, details] of util.getSystemErrorMap().entries()) {
      if (details[0] === code) {
        (error as SystemError).errno = errno;
        break;
      }
    }
  }

  throw error;
}
