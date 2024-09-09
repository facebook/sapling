/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {execa} from 'execa';
import * as fs from 'node:fs';
import {join} from 'node:path';
import {dirSync} from 'tmp';

const logger = console;
const keepTmp = process.env.KEEP != null;

/**
 * Maintains the lifecyle of a test repo.
 * Add convinent methods to modify the repo here.
 */
export class TestRepo {
  /** Creates a new temporary repo. The repo will be deleted on exit. */
  static async new(repoName = 'example', command = process.env.SL ?? 'sl'): Promise<TestRepo> {
    const tmpDir = dirSync({unsafeCleanup: true});
    if (!keepTmp) {
      process.on('exit', () => tmpDir.removeCallback());
    }
    const repoPath = join(tmpDir.name, repoName);
    logger.info(keepTmp ? '' : 'Temporary', 'repo path:', repoPath);
    fs.mkdirSync(repoPath);
    const repo = new TestRepo(repoPath, command);
    await repo.run(['init', '--git']);
    return repo;
  }

  /** Adds commits via the `debugdrawdag` command. */
  async drawdag(ascii = '', script?: string) {
    let input = ascii;
    if (script != null) {
      input += `\npython:\n${script}\n`;
    }
    await this.run(['debugdrawdag', '--no-bookmarks'], input);
  }

  /** Runs command in the repo. Returns its stdout. */
  async run(args: Array<string>, input = ''): Promise<string> {
    const env = {...process.env, SL_AUTOMATION: '1', HGPLAIN: '1'};
    logger.info('Running', this.command, args.join(' '));
    const child = await execa(this.command, args, {
      cwd: this.repoPath,
      input: Buffer.from(input),
      env,
    });
    logger.debug('Ran', this.command, args.join(' '), '. Exit code:', child.exitCode);
    return child.stdout;
  }

  /**
   * Spawns a headless ISL server and returns the URL to access it.
   * The server will be killed on exit.
   */
  async serveUrl(): Promise<string> {
    const out = await this.run(['web', '--no-open', '--json']);
    const parsed = JSON.parse(out) as {url: string; pid?: number};
    const {url, pid} = parsed;
    if (!keepTmp && pid != null && pid > 0) {
      process.on('exit', () => process.kill(pid));
    }
    return url;
  }

  constructor(private repoPath: string, private command: string) {}
}
