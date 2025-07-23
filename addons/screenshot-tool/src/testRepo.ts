/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {spawn} from 'node:child_process';
import * as fs from 'node:fs/promises';
import {join} from 'node:path';
import {dirSync} from 'tmp';
import {getCacheDir, sha1} from './utils';

const logger = console;
const keepTmp = process.env.KEEP != null;

/**
 * Maintains the lifecycle of a test repo.
 * Add convenient methods to modify the repo here.
 */
export class TestRepo {
  /**
   * Creates a new temporary directory.
   * The directory will be deleted on exit, unless KEEP is set.
   * This does not initialize the repo. Use `init()` to initialize it.
   */
  static async new(repoName = 'example', command = process.env.SL ?? 'sl'): Promise<TestRepo> {
    const tmpDir = dirSync({unsafeCleanup: true});
    if (!keepTmp) {
      process.on('exit', () => tmpDir.removeCallback());
    }
    const repoPath = join(tmpDir.name, repoName);
    logger.info(keepTmp ? '' : 'Temporary', 'repo path:', repoPath);
    await fs.mkdir(repoPath);
    const repo = new TestRepo(repoPath, command);
    return repo;
  }

  /** Initialize the repo. */
  async init(): Promise<void> {
    await this.run(['init', '--config=format.use-eager-repo=true']);
  }

  /**
   * Cache the side effect of `func` to this repo.
   * After executing `func`, the files in the repo will be copied to a cache directory.
   * The next time the same `func` is passed here, restore the files from the cache
   * directory without running `func`.
   */
  async cached(func: (repo: TestRepo) => Promise<void>, cacheKey?: string): Promise<void> {
    const hash = sha1(cacheKey ?? func.toString());
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

  /** Set repo-level configs. `configs` are in "foo.bar=baz" form. */
  async setConfig(configs: Array<string>) {
    await this.run(['config', '--local', ...configs]);
  }

  /** Adds commits via the `debugdrawdag` command. */
  async drawdag(ascii = '', script?: string) {
    let input = ascii;
    if (script != null) {
      input += `\npython:\n${dedent(script)}\n`;
    }
    await this.run(['debugdrawdag', '--no-bookmarks'], input);
  }

  /** Import patches from the given file. */
  async import(patchFile: string) {
    await this.run(['import', patchFile]);
  }

  /** Runs command in the repo. Returns its stdout. */
  run(args: Array<string>, input = ''): Promise<string> {
    const env = {...process.env, SL_AUTOMATION: '1', HGPLAIN: '1', NOSCMLOG: '1'};
    logger.info('Running', this.command, args.join(' '));
    const child = spawn(this.command, args, {
      cwd: this.repoPath,
      stdio: ['pipe', 'pipe', 'inherit'],
      shell: false,
      windowsHide: true,
      env,
    });
    child.stdin?.write(Buffer.from(input));
    child.stdin?.end();
    const stdout: string[] = [];
    child.stdout.on('data', data => stdout.push(data.toString()));
    return new Promise((resolve, _reject) => {
      child.on('close', (code, _signal) => {
        logger.debug('Ran', this.command, args.join(' '), '. Exit code:', code);
        resolve(stdout.join(''));
      });
    });
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

  constructor(
    private repoPath: string,
    private command: string,
  ) {}
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

/** Remove common prefix spaces for non-empty lines. */
function dedent(s: string): string {
  const lines = s.split('\n');
  const indent = Math.min(
    ...lines.filter(l => l.trim().length > 0).map(l => l.length - l.trimStart().length),
  );
  const newLines = lines.map(l => l.slice(indent));
  return newLines.join('\n');
}
