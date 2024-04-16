/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepositoryContext} from './serverTypes';
import type {AbsolutePath, MergeConflicts} from 'isl/src/types';

import {isExecaError} from './utils';
import execa from 'execa';
import os from 'os';

export const MAX_FETCHED_FILES_PER_COMMIT = 25;
export const MAX_SIMULTANEOUS_CAT_CALLS = 4;
/** Timeout for non-operation commands. Operations like goto and rebase are expected to take longer,
 * but status, log, cat, etc should typically take <10s. */
export const READ_COMMAND_TIMEOUT_MS = 40_000;

export type ConflictFileData = {
  contents: string;
  exists: boolean;
  isexec: boolean;
  issymlink: boolean;
};
export type ResolveCommandConflictOutput = [
  | {
      command: null;
      conflicts: [];
      pathconflicts: [];
    }
  | {
      command: string;
      command_details: {cmd: string; to_abort: string; to_continue: string};
      conflicts: Array<{
        base: ConflictFileData;
        local: ConflictFileData;
        output: ConflictFileData;
        other: ConflictFileData;
        path: string;
      }>;
      pathconflicts: Array<never>;
    },
];

/** Run an sl command (without analytics). */
export async function runCommand(
  ctx: RepositoryContext,
  args_: Array<string>,
  options_?: execa.Options,
  timeout: number = READ_COMMAND_TIMEOUT_MS,
): Promise<execa.ExecaReturnValue<string>> {
  const {command, args, options} = getExecParams(ctx.cmd, args_, ctx.cwd, options_);
  ctx.logger.log('run command: ', command, args[0]);
  const result = execa(command, args, options);

  let timedOut = false;
  let timeoutId: NodeJS.Timeout | undefined;
  if (timeout > 0) {
    timeoutId = setTimeout(() => {
      result.kill('SIGTERM', {forceKillAfterTimeout: 5_000});
      ctx.logger.error(`Timed out waiting for ${command} ${args[0]} to finish`);
      timedOut = true;
    }, timeout);
    result.on('exit', () => {
      clearTimeout(timeoutId);
    });
  }

  try {
    const val = await result;
    return val;
  } catch (err: unknown) {
    if (isExecaError(err)) {
      if (err.killed) {
        if (timedOut) {
          throw new Error('Timed out');
        }
        throw new Error('Killed');
      }
    }
    ctx.logger.error(`Error running ${command} ${args[0]}: ${err?.toString()}`);
    throw err;
  } finally {
    clearTimeout(timeoutId);
  }
}

/**
 * Root of the repository where the .sl folder lives.
 * Throws only if `command` is invalid, so this check can double as validation of the `sl` command */
export async function findRoot(ctx: RepositoryContext): Promise<AbsolutePath | undefined> {
  try {
    return (await runCommand(ctx, ['root'])).stdout;
  } catch (error) {
    if (
      ['ENOENT', 'EACCES'].includes((error as {code: string}).code) ||
      // On Windows, we won't necessarily get an actual ENOENT error code in the error,
      // because execa does not attempt to detect this.
      // Other spawning libraries like node-cross-spawn do, which is the approach we can take.
      // We can do this because we know how `root` uses exit codes.
      // https://github.com/sindresorhus/execa/issues/469#issuecomment-859924543
      (os.platform() === 'win32' && (error as {exitCode: number}).exitCode === 1)
    ) {
      ctx.logger.error(`command ${ctx.cmd} not found`, error);
      throw error;
    }
  }
}

export async function findDotDir(ctx: RepositoryContext): Promise<AbsolutePath | undefined> {
  try {
    return (await runCommand(ctx, ['root', '--dotdir'])).stdout;
  } catch (error) {
    ctx.logger.error(`Failed to find repository dotdir in ${ctx.cwd}`, error);
    return undefined;
  }
}

/**
 * Read multiple configs.
 * Return a Map from config name to config value for present configs.
 * Missing configs will not be returned.
 * Errors are silenced.
 */
export async function getConfigs<T extends string>(
  ctx: RepositoryContext,
  configNames: ReadonlyArray<T>,
): Promise<Map<T, string>> {
  if (configOverride !== undefined) {
    // Use the override to answer config questions.
    const configMap = new Map(
      configNames.flatMap(name => {
        const value = configOverride?.get(name);
        return value === undefined ? [] : [[name, value]];
      }),
    );
    return configMap;
  }
  const configMap: Map<T, string> = new Map();
  try {
    // config command does not support multiple configs yet, but supports multiple sections.
    // (such limitation makes sense for non-JSON output, which can be ambigious)
    // TODO: Remove this once we can validate that OSS users are using a new enough Sapling version.
    const sections = new Set<string>(configNames.flatMap(name => name.split('.').at(0) ?? []));
    const result = await runCommand(ctx, ['config', '-Tjson'].concat([...sections]));
    const configs: [{name: T; value: string}] = JSON.parse(result.stdout);
    for (const config of configs) {
      configMap.set(config.name, config.value);
    }
  } catch (e) {
    ctx.logger.error(`failed to read configs from ${ctx.cwd}: ${e}`);
  }
  ctx.logger.info(`loaded configs from ${ctx.cwd}:`, configMap);
  return configMap;
}

export type ConfigLevel = 'user' | 'system' | 'local';
export async function setConfig(
  ctx: RepositoryContext,
  level: ConfigLevel,
  configName: string,
  configValue: string,
): Promise<void> {
  await runCommand(ctx, ['config', `--${level}`, configName, configValue]);
}

export function getExecParams(
  command: string,
  args_: Array<string>,
  cwd: string,
  options_?: execa.Options,
  env?: NodeJS.ProcessEnv | Record<string, string>,
): {
  command: string;
  args: Array<string>;
  options: execa.Options;
} {
  let args = [...args_, '--noninteractive'];
  // expandHomeDir is not supported on windows
  if (process.platform !== 'win32') {
    // commit/amend have unconventional ways of escaping slashes from messages.
    // We have to 'unescape' to make it work correctly.
    args = args.map(arg => arg.replace(/\\\\/g, '\\'));
  }
  const [commandName] = args;
  if (EXCLUDE_FROM_BLACKBOX_COMMANDS.has(commandName)) {
    args.push('--config', 'extensions.blackbox=!');
  }
  const newEnv = {
    ...options_?.env,
    ...env,
    // TODO: remove when SL_ENCODING is used everywhere
    HGENCODING: 'UTF-8',
    SL_ENCODING: 'UTF-8',
    // override any custom aliases a user has defined.
    SL_AUTOMATION: 'true',
    // allow looking up diff numbers even in plain mode.
    // allow constructing the `.git/sl` repo regardless of the identity.
    SL_AUTOMATION_EXCEPT: 'ghrevset,phrevset,sniff',
    // Prevent user-specified merge tools from attempting to
    // open interactive editors.
    HGMERGE: ':merge3',
    SL_MERGE: ':merge3',
    EDITOR: undefined,
    VISUAL: undefined,
    HGUSER: undefined,
    HGEDITOR: undefined,
  } as unknown as NodeJS.ProcessEnv;
  let langEnv = newEnv.LANG ?? process.env.LANG;
  if (langEnv === undefined || !langEnv.toUpperCase().endsWith('UTF-8')) {
    langEnv = 'C.UTF-8';
  }
  newEnv.LANG = langEnv;
  const options: execa.Options = {
    ...options_,
    env: newEnv,
    cwd,
  };

  if (args_[0] === 'status') {
    // Take a lock when running status so that multiple status calls in parallel don't overload watchman
    args.push('--config', 'fsmonitor.watchman-query-lock=True');
  }

  // TODO: we could run with systemd for better OOM protection when on linux
  return {command, args, options};
}

// Avoid spamming the blackbox with read-only commands.
const EXCLUDE_FROM_BLACKBOX_COMMANDS = new Set(['cat', 'config', 'diff', 'log', 'show', 'status']);

/**
 * extract repo info from a remote url, typically for GitHub or GitHub Enterprise,
 * in various formats:
 * https://github.com/owner/repo
 * https://github.com/owner/repo.git
 * github.com/owner/repo.git
 * git@github.com:owner/repo.git
 * ssh:git@github.com:owner/repo.git
 * ssh://git@github.com/owner/repo.git
 * git+ssh:git@github.com:owner/repo.git
 *
 * or similar urls with GitHub Enterprise hostnames:
 * https://ghe.myCompany.com/owner/repo
 */
export function extractRepoInfoFromUrl(
  url: string,
): {repo: string; owner: string; hostname: string} | null {
  const match =
    /(?:https:\/\/(.*)\/|(?:git\+ssh:\/\/|ssh:\/\/)?(?:git@)?([^:/]*)[:/])([^/]+)\/(.+?)(?:\.git)?$/.exec(
      url,
    );

  if (match == null) {
    return null;
  }

  const [, hostname1, hostname2, owner, repo] = match;
  return {owner, repo, hostname: hostname1 ?? hostname2};
}

export function computeNewConflicts(
  previousConflicts: MergeConflicts,
  commandOutput: ResolveCommandConflictOutput,
  fetchStartTimestamp: number,
): MergeConflicts | undefined {
  const newConflictData = commandOutput?.[0];
  if (newConflictData?.command == null) {
    return undefined;
  }

  const conflicts: MergeConflicts = {
    state: 'loaded',
    command: newConflictData.command,
    toContinue: newConflictData.command_details.to_continue,
    toAbort: newConflictData.command_details.to_abort,
    files: [],
    fetchStartTimestamp,
    fetchCompletedTimestamp: Date.now(),
  };

  const previousFiles = previousConflicts?.files ?? [];

  const newConflictSet = new Set(newConflictData.conflicts.map(conflict => conflict.path));
  const previousFilesSet = new Set(previousFiles.map(file => file.path));
  const newlyAddedConflicts = new Set(
    [...newConflictSet].filter(file => !previousFilesSet.has(file)),
  );
  // we may have seen conflicts before, some of which might now be resolved.
  // Preserve previous ordering by first pulling from previous files
  conflicts.files = previousFiles.map(conflict =>
    newConflictSet.has(conflict.path)
      ? {path: conflict.path, status: 'U'}
      : // 'R' is overloaded to mean "removed" for `sl status` but 'Resolved' for `sl resolve --list`
        // let's re-write this to make the UI layer simpler.
        {path: conflict.path, status: 'Resolved'},
  );
  if (newlyAddedConflicts.size > 0) {
    conflicts.files.push(
      ...[...newlyAddedConflicts].map(conflict => ({path: conflict, status: 'U' as const})),
    );
  }

  return conflicts;
}

/**
 * By default, detect "jest" and enable config override to avoid shelling out.
 * See also `getConfigs`.
 */
let configOverride: undefined | Map<string, string> =
  typeof jest === 'undefined' ? undefined : new Map();

/**
 * Set the "knownConfig" used by new repos.
 * This is useful in tests and prevents shelling out to config commands.
 */
export function setConfigOverrideForTests(configs: Iterable<[string, string]>, override = true) {
  if (override) {
    configOverride = new Map(configs);
  } else {
    configOverride ??= new Map();
    for (const [key, value] of configs) {
      configOverride.set(key, value);
    }
  }
}
