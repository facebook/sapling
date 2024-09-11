/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {getCacheDir, sha1} from './utils';
import {execa} from 'execa';
import * as fs from 'node:fs/promises';
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
    await fs.mkdir(repoPath);
    const repo = new TestRepo(repoPath, command);
    await repo.run(['init', '--git']);
    return repo;
  }

  /**
   * Cache the side effect of `func` to this repo.
   * After executing `func`, the files in the repo will be copied to a cache directory.
   * The next time the same `func` is passed here, restore the files from the cache
   * directory without running `func`.
   */
  async cached(func: (repo: TestRepo) => Promise<void>): Promise<void> {
    const hash = sha1(func.toString());
    const cacheDir = join(await getCacheDir('repos'), hash.substring(0, 8));
    let cacheExists: boolean;
    try {
      const stat = await fs.stat(cacheDir);
      cacheExists = stat.isDirectory();
    } catch {
      cacheExists = false;
    }
    if (cacheExists) {
      logger.info(`Reusing cached repo at ${cacheDir}`);
      await copyRecursive(cacheDir, this.repoPath);
    } else {
      await func(this);
      logger.info(`Backing up repo to ${cacheDir}`);
      await copyRecursive(this.repoPath, cacheDir);
    }
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

async function copyRecursive(src: string, dst: string): Promise<void> {
  const srcStats = await fs.lstat(src);
  if (srcStats.isDirectory()) {
    await fs.mkdir(dst, {recursive: true});
    const items = await fs.readdir(src);
    await Promise.all(
      items.map(async item => {
        const srcItemPath = join(src, item);
        const dstItemPath = join(dst, item);
        await copyRecursive(srcItemPath, dstItemPath);
      }),
    );
  } else if (srcStats.isFile()) {
    await fs.copyFile(src, dst);
  }
}
