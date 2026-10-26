/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {localImageRoots, readLocalImage} from '../localImage';

describe('localImage', () => {
  let workspace: string;
  let repoRoot: string;

  beforeEach(() => {
    workspace = fs.mkdtempSync(path.join(os.tmpdir(), 'isl-local-image-'));
    repoRoot = path.join(workspace, 'repo');
    fs.mkdirSync(path.join(repoRoot, 'docs'), {recursive: true});
    fs.mkdirSync(path.join(workspace, 'scratch'));
    fs.writeFileSync(path.join(repoRoot, 'docs', 'shot.png'), Buffer.from([1, 2, 3]));
    fs.writeFileSync(path.join(workspace, 'scratch', 'outside.png'), Buffer.from([4, 5, 6]));
    fs.writeFileSync(path.join(repoRoot, 'notes.txt'), 'not an image');
  });

  afterEach(() => {
    fs.rmSync(workspace, {recursive: true, force: true});
  });

  test('reads an image under the repo root as a data URL', async () => {
    await expect(readLocalImage([repoRoot], 'docs/shot.png')).resolves.toEqual(
      'data:image/png;base64,AQID',
    );
  });

  test('resolves against a configured root, including a ~ prefix', async () => {
    const roots = localImageRoots(repoRoot, ` .. , ~/elsewhere`);
    expect(roots).toEqual([repoRoot, workspace, path.join(os.homedir(), 'elsewhere')]);
    await expect(readLocalImage(roots, 'scratch/outside.png')).resolves.toEqual(
      'data:image/png;base64,BAUG',
    );
  });

  test('refuses paths that leave every root', async () => {
    await expect(readLocalImage([repoRoot], '../scratch/outside.png')).rejects.toThrow(
      'Image not found',
    );
    await expect(
      readLocalImage([repoRoot], path.join(workspace, 'scratch', 'outside.png')),
    ).rejects.toThrow('must be relative');
  });

  test('refuses a symlink that points outside the root', async () => {
    fs.symlinkSync(
      path.join(workspace, 'scratch', 'outside.png'),
      path.join(repoRoot, 'docs', 'link.png'),
    );
    await expect(readLocalImage([repoRoot], 'docs/link.png')).rejects.toThrow('Image not found');
  });

  test('refuses files that are not images', async () => {
    await expect(readLocalImage([repoRoot], 'notes.txt')).rejects.toThrow('Not an image');
  });
});
