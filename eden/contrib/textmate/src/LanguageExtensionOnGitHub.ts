/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ExecFileOptions} from 'child_process';

import AbstractLanguageExtension from './AbstractLanguageExtension';
import child_process, {execFile} from 'child_process';
import {promises as fs} from 'fs';
import osMod from 'os';
import pathMod from 'path';

type ConstructorArgs = {
  /** GitHub organization for repo. */
  organization: string;

  /** GitHub project for repo. */
  project: string;

  /**
   * Full hex hash of commit at which to fetch files. (Preferred over branch
   * names that could point to a different commit over time, which makes them
   * unstable identifiers.)
   */
  commit: string;

  /**
   * path within project where root of the extension can be found; must be empty
   * string or end with a slash.
   */
  path?: string;

  /**
   * If not null, does a local clone of the GitHub repo checked out at the
   * specified commit and runs this build command, which must be an array of
   * strings that indicates the build command to run, such as `['yarn', 'run',
   * 'build:grammar']`. Note that `yarn install` is always run before the
   * `buildCommand` if it is set. After the build command finishes, all calls to
   * `getContents()` will read from the local clone rather than making curl
   * requests to GitHub.
   */
  buildCommand?: string[] | null;

  /**
   * If set, will be included as an environment variable when making calls to
   * `curl` or `git`.
   */
  https_proxy?: string | null;
};

type BuildOperation = {
  uri: string;
  commit: string;
  command: string[];
  tmpPrefix: string;
  cloneOperation: Promise<string> | null;
};

/**
 * Represents a VS Code language extension whose source is in a public GitHub
 * repo.
 */
export default class LanguageExtensionOnGitHub extends AbstractLanguageExtension {
  private _id: string;
  private _baseUrl: string;
  private _path: string;
  private _https_proxy: string | null;
  private _build: BuildOperation | null;

  constructor({
    organization,
    project,
    commit,
    path = '',
    https_proxy = null,
    buildCommand = null,
  }: ConstructorArgs) {
    super();
    this._id = `https://github.com/${organization}/${project}/tree/${commit}/${path}`;
    this._baseUrl = `https://github.com/${organization}/${project}/raw/${commit}`;
    this._path = path;
    this._https_proxy = https_proxy;
    this._build =
      buildCommand == null
        ? null
        : {
            uri: `https://github.com/${organization}/${project}`,
            commit,
            command: buildCommand,
            tmpPrefix: `${organization}-${project}`,
            // When updated, will be a Promise<string> where the string is the
            // directory where the Git clone was created.
            cloneOperation: null,
          };
  }

  async getContents(pathRelativeToExtensionRoot: string): Promise<string> {
    if (this._build != null) {
      const cloneDir = await getCloneDir(this._build, this._https_proxy);
      const fullPath = pathMod.join(cloneDir, pathRelativeToExtensionRoot);
      return fs.readFile(fullPath, {encoding: 'utf8'});
    } else {
      // Special thanks to the folks at Microsoft for naming a file
      // "Regular Expressions (JavaScript).tmLanguage", which is why we need
      // to use encodeURIComponent().
      const repoRelativePath = encodeURIComponent(
        pathMod.normalize(pathMod.join(this._path, pathRelativeToExtensionRoot)),
      );
      const url = `${this._baseUrl}/${repoRelativePath}`;
      return httpGet(url, this._https_proxy);
    }
  }

  toString(): string {
    return this._id;
  }
}

/**
 * Creates the clone operation if it does not already exist and returns the
 * corresponding Promise<string>.
 */
function getCloneDir(build: BuildOperation, https_proxy: string | null): Promise<string> {
  if (build.cloneOperation == null) {
    build.cloneOperation = gitClone(build, https_proxy);
  }
  return build.cloneOperation;
}

/**
 * Executes the `git clone`, `yarn install`, and build command. Returns the
 * tmp directory where the Git clone was created.
 */
async function gitClone(build: BuildOperation, https_proxy: string | null): Promise<string> {
  const {command, commit, tmpPrefix, uri} = build;
  const tmp = await fs.mkdtemp(pathMod.join(osMod.tmpdir(), tmpPrefix));
  // eslint-disable-next-line no-console
  console.info(`creating 'git clone' in ${tmp}`);
  const env = {...process.env};
  if (https_proxy != null) {
    env.https_proxy = https_proxy;
  }
  const options = {env};

  await execFileAsync('git', ['clone', '--no-checkout', uri, tmp], options);
  await execFileAsync('git', ['-C', tmp, 'checkout', commit], options);
  // Should check if there is an npm.lock or a yarn.lock!
  await execFileAsync('yarn', ['install'], {cwd: tmp});
  await execFileAsync(command[0], command.slice(1), {cwd: tmp});
  return tmp;
}

/** Promisified version of child_process.execFile(). */
function execFileAsync(
  file: string,
  args: string[],
  options: ExecFileOptions,
): Promise<string | Buffer> {
  return new Promise((resolve, reject) => {
    execFile(file, args, options, (error, stdout, stderr) => {
      if (error == null) {
        resolve(stdout);
      } else {
        // eslint-disable-next-line no-console
        console.error(stderr);
        reject(error);
      }
    });
  });
}

function httpGet(url: string, https_proxy: string | null): Promise<string> {
  // We had trouble getting standard HTTP libraries in Node to play well the
  // https_proxy environment variable, so we use curl even though it's a gross
  // solution.
  return new Promise((resolve, reject) => {
    const stdout: Uint8Array[] = [];
    const stderr: Uint8Array[] = [];
    const env = {...process.env};
    if (https_proxy != null) {
      env.https_proxy = https_proxy;
    }
    const options = {env};

    const child = child_process.spawn('curl', ['--fail', '--location', url], options);
    child.stdout.on('data', data => {
      stdout.push(data);
    });
    child.stderr.on('data', data => {
      stderr.push(data);
    });
    child.on('close', code => {
      if (code === 0) {
        resolve(Buffer.concat(stdout).toString('utf8'));
      } else {
        const error = Buffer.concat(stderr).toString('utf8');
        reject(`failed to fetch ${url}: ${error}`);
      }
    });
  });
}
