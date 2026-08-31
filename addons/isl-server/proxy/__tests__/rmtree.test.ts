/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {exists} from 'shared/fs';
import rmtree from '../rmtree';

describe('rmtree', () => {
  afterEach(() => {
    jest.restoreAllMocks();
  });

  it('does not complain about a non-existent file', async () => {
    const tmp = await makeTmpDir();
    await rmtree(path.join(tmp, 'foo'));
  });

  it('removes a file', async () => {
    const tmp = await makeTmpDir();
    const file = path.join(tmp, 'foo');
    await fs.promises.writeFile(file, 'foobar');
    await rmtree(file);

    expect(await exists(file)).toBe(false);
  });

  it('removes an empty folder', async () => {
    const tmp = await makeTmpDir();
    const folder = path.join(tmp, 'foo');
    await fs.promises.mkdir(folder);
    await rmtree(folder);

    expect(await exists(folder)).toBe(false);
  });

  it('removes a folder with files', async () => {
    const tmp = await makeTmpDir();
    const folder = path.join(tmp, 'foo');
    await fs.promises.mkdir(folder);
    await fs.promises.writeFile(path.join(folder, '1'), '1');
    await fs.promises.writeFile(path.join(folder, '2'), '2');
    await fs.promises.writeFile(path.join(folder, '3'), '3');
    await fs.promises.writeFile(path.join(folder, '4'), '4');
    await rmtree(folder);

    expect(await exists(folder)).toBe(false);
  });

  it('removes a deeper tree of folders and files', async () => {
    const folder = 'tree';
    const child = path.join(folder, 'child');
    const grandchild = path.join(child, 'grandchild');
    const directory = {isDirectory: () => true} as fs.Stats;
    const file = {isDirectory: () => false} as fs.Stats;
    jest
      .spyOn(fs.promises, 'lstat')
      .mockResolvedValueOnce(directory)
      .mockResolvedValueOnce(file)
      .mockResolvedValueOnce(directory)
      .mockResolvedValueOnce(file)
      .mockResolvedValueOnce(directory)
      .mockResolvedValueOnce(file);
    const readdir = jest.spyOn(fs.promises, 'readdir') as unknown as jest.MockedFunction<
      (folder: string) => Promise<Array<string>>
    >;
    readdir
      .mockResolvedValueOnce(['root-file', 'child'])
      .mockResolvedValueOnce(['child-file', 'grandchild'])
      .mockResolvedValueOnce(['deep-file'])
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([]);
    const unlink = jest.spyOn(fs.promises, 'unlink').mockResolvedValue(undefined);
    const rmdir = jest.spyOn(fs.promises, 'rmdir').mockResolvedValue(undefined);

    await rmtree(folder);

    expect(unlink.mock.calls.map(([file]) => file)).toEqual([
      path.join(folder, 'root-file'),
      path.join(child, 'child-file'),
      path.join(grandchild, 'deep-file'),
    ]);
    expect(rmdir.mock.calls.map(([folder]) => folder)).toEqual([grandchild, child, folder]);
  });

  it('does not follow argument if it is a symlink', async () => {
    const tmp = await makeTmpDir();
    const target = path.join(tmp, 'target');
    const link = path.join(tmp, 'link');
    await fs.promises.writeFile(target, 'target file');
    await fs.promises.symlink(target, link);
    expect(await fs.promises.readFile(link, {encoding: 'utf8'})).toBe('target file');
    expect(await exists(link)).toBe(true);
    await rmtree(link);

    expect(await exists(link)).toBe(false);
    expect(await exists(target)).toBe(true);
  });

  it('does not follow symlink in the tree', async () => {
    const tmp = await makeTmpDir();
    const target = path.join(tmp, 'target');
    await fs.promises.writeFile(target, 'target file');

    const folder = path.join(tmp, 'folder');
    await fs.promises.mkdir(folder);
    const link = path.join(folder, 'link');
    await fs.promises.symlink(target, link);
    expect(await fs.promises.readFile(link, {encoding: 'utf8'})).toBe('target file');
    await rmtree(folder);

    expect(await exists(folder)).toBe(false);
    expect(await exists(target)).toBe(true);
  });
});

function makeTmpDir(): Promise<string> {
  return fs.promises.mkdtemp(path.join(os.tmpdir(), 'rmtree-test'));
}
