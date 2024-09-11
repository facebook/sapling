/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import crypto from 'node:crypto';
import fs from 'node:fs';
import {homedir} from 'node:os';
import {join} from 'node:path';

export function sha1(str: string): string {
  return crypto.createHash('sha1').update(str).digest('hex');
}

export async function getCacheDir(subdir?: string): Promise<string> {
  let dir = join(homedir(), '.cache', 'isl-screenshot');
  if (subdir != null) {
    dir = join(dir, subdir);
  }
  await fs.promises.mkdir(dir, {recursive: true});
  return dir;
}
