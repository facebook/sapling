/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {safeWhichUsingEnvironment} from '../safeWhich';
import fs from 'fs';
import os from 'os';
import path from 'path';

const exeNoExtension = 'notepad';
const DEFAULT_PATHEXT = [
  '.COM',
  '.EXE',
  '.BAT',
  '.CMD',
  '.VBS',
  '.VBE',
  '.JS',
  '.JSE',
  '.WSF',
  '.WSH',
  '.MSC',
].join(path.delimiter);

describe('safeWhichUsingEnvironment', () => {
  let tempDirs: string[] = [];

  beforeEach(async () => {
    const tmp0 = await createTempDir();
    const tmp1 = await createTempDir();
    const tmp2 = await createTempDir();
    tempDirs = [tmp0, tmp1, tmp2];
  });

  afterEach(async () => {
    await removeTempDir(tempDirs[0]);
    await removeTempDir(tempDirs[1]);
    await removeTempDir(tempDirs[2]);
  });

  test('executable not on PATH', async () => {
    const [tmp0, tmp1, tmp2] = tempDirs;
    const PATH = createPathEnv(tmp0, tmp1, tmp2);
    await expect(
      safeWhichUsingEnvironment({exe: exeNoExtension, PATH, PATHEXT: DEFAULT_PATHEXT}),
    ).rejects.toEqual(
      expect.objectContaining({
        code: 'ENOENT',
      }),
    );
  });
});

function createPathEnv(...items: string[]): string {
  return items.join(path.delimiter);
}

function createTempDir(): Promise<string> {
  return fs.promises.mkdtemp(path.join(os.tmpdir(), 'sapling-which-test-'));
}

async function removeTempDir(directory: string): Promise<void> {
  const entries = await fs.promises.readdir(directory);
  // In this test, all entries are files, not folders.
  for (const file of entries) {
    // eslint-disable-next-line no-await-in-loop
    await fs.promises.unlink(path.join(directory, file));
  }
  await fs.promises.rmdir(directory);
}
