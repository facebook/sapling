/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Example, TestRepo} from '../example';

import {dirname, join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {BASE_EXAMPLE} from '../example';

const __dirname = dirname(fileURLToPath(import.meta.url));

export const EXAMPLE: Example = {
  ...BASE_EXAMPLE,
  async populateRepo(repo: TestRepo) {
    const patchPath = join(__dirname, 'linelog.ts.patch');
    await repo.import(patchPath);
  },
  postOpenISL(): Promise<void> {
    return this.repl();
  },
  openISLOptions: {
    ...BASE_EXAMPLE.openISLOptions,
    now: 1683084584,
  },
};
