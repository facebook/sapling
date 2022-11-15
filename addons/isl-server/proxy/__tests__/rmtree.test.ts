/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import rmtree from '../rmtree';
import fs from 'fs';
import os from 'os';
import path from 'path';
import {exists} from 'shared/fs';

describe('rmtree', () => {
  let tmp: string;

  beforeEach(async () => {
    tmp = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'rmtree-test'));
  });

  it('does not complain about a non-existent file', async () => {
    await rmtree(path.join(tmp, 'foo'));
  });

  it('removes a file', async () => {
    const file = path.join(tmp, 'foo');
    await fs.promises.writeFile(file, 'foobar');
    await rmtree(file);

    expect(await exists(file)).toBe(false);
  });

  it('removes an empty folder', async () => {
    const folder = path.join(tmp, 'foo');
    await fs.promises.mkdir(folder);
    await rmtree(folder);

    expect(await exists(folder)).toBe(false);
  });

  it('removes a folder with files', async () => {
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
    const folder = path.join(tmp, 'tree');
    await fs.promises.mkdir(path.join(folder, '1/2/3/4/5'), {recursive: true});
    await fs.promises.writeFile(path.join(folder, '1/A'), 'A');
    await fs.promises.writeFile(path.join(folder, '1/2/B'), 'B');
    await fs.promises.writeFile(path.join(folder, '1/2/B'), 'B');
    await fs.promises.writeFile(path.join(folder, '1/2/3/C'), 'C');
    await fs.promises.writeFile(path.join(folder, '1/2/3/4/D'), 'D');
    await fs.promises.writeFile(path.join(folder, '1/2/3/4/5/E'), 'E');
    await rmtree(folder);

    expect(await exists(folder)).toBe(false);
  });

  it('does not follow argument if it is a symlink', async () => {
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
