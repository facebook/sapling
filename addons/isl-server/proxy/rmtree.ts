/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import fs from 'fs';
import path from 'path';

/**
 * fs.promises.rm() was introduced in Node v14.14.0, so to in order to run in
 * Node v10, we must provide our own implementation.
 *
 * This functions like `rm -rf <file>`.
 */
export default async function rmtree(file: string): Promise<void> {
  let stat;
  try {
    stat = await fs.promises.lstat(file);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') {
      // If the file does not exist, nothing to do!
      return;
    } else {
      throw error;
    }
  }

  if (stat.isDirectory()) {
    const stack = [file];
    while (stack.length !== 0) {
      // eslint-disable-next-line no-await-in-loop
      await rmtreeIterative(stack);
    }
  } else {
    await fs.promises.unlink(file);
  }
}

/**
 * fs.promises.rm() was introduced in Node v14.14.0, so to in order to run in
 * Node v10, we must provide our own implementation.
 *
 * @param stack a list of folders to remove recursively. Folders at the end of
 *   the array will be removed before preceding folders.
 */
async function rmtreeIterative(stack: Array<string>): Promise<void> {
  // This is effectively a "peek" on the stack.
  const folder = stack[stack.length - 1];
  if (folder == null) {
    throw new Error(`invariant violation: empty stack`);
  }

  // We rely on the caller to ensure `folder` is a path to a directory rather
  // than stat each argument that was passed in.
  const files = await fs.promises.readdir(folder);

  const stackLength = stack.length;
  await Promise.all(
    files.map(async (file: string) => {
      const fullPath = path.join(folder, file);
      const stat = await fs.promises.lstat(fullPath);
      if (stat.isDirectory()) {
        stack.push(fullPath);
      } else {
        await fs.promises.unlink(fullPath);
      }
    }),
  );

  // If nothing was pushed onto the stack, then we can assume this folder is
  // now empty and rmdir() will succeed.
  if (stack.length === stackLength) {
    await fs.promises.rmdir(folder);
    stack.pop();
  }
}
