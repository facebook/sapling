/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CodeReviewProvider} from './CodeReviewProvider';
import type {ServerSideTracker} from './analytics/serverSideTracker';
import type {Logger} from './logger';
import type {
  CommitInfo,
  CommitPhaseType,
  Disposable,
  CommandArg,
  SmartlogCommits,
  SuccessorInfo,
  UncommittedChanges,
  ChangedFile,
  RepoInfo,
  OperationCommandProgressReporter,
  AbsolutePath,
  RunnableOperation,
  OperationProgress,
  DiffId,
  PageVisibility,
  MergeConflicts,
  ValidatedRepoInfo,
  CodeReviewSystem,
  Revset,
  PreferredSubmitCommand,
  RepoRelativePath,
  FetchedUncommittedChanges,
  FetchedCommits,
} from 'isl/src/types';

import {Internal} from './Internal';
import {OperationQueue} from './OperationQueue';
import {PageFocusTracker} from './PageFocusTracker';
import {WatchForChanges} from './WatchForChanges';
import {DEFAULT_DAYS_OF_COMMITS_TO_LOAD, ErrorShortMessages} from './constants';
import {GitHubCodeReviewProvider} from './github/githubCodeReviewProvider';
import {isGithubEnterprise} from './github/queryGraphQL';
import {handleAbortSignalOnProcess, serializeAsyncCall} from './utils';
import execa from 'execa';
import {CommandRunner} from 'isl/src/types';
import os from 'os';
import path from 'path';
import {RateLimiter} from 'shared/RateLimiter';
import {TypedEventEmitter} from 'shared/TypedEventEmitter';
import {exists} from 'shared/fs';
import {removeLeadingPathSep} from 'shared/pathUtils';
import {notEmpty, unwrap} from 'shared/utils';

export const COMMIT_END_MARK = '<<COMMIT_END_MARK>>';
export const NULL_CHAR = '\0';
const ESCAPED_NULL_CHAR = '\\0';

const NO_NODE_HASH = '000000000000';
const HEAD_MARKER = '@';
const MAX_FETCHED_FILES_PER_COMMIT = 25;
const MAX_SIMULTANEOUS_CAT_CALLS = 4;

const FIELDS = {
  hash: '{node|short}',
  title: '{desc|firstline}',
  author: '{author}',
  date: '{date|isodatesec}',
  phase: '{phase}',
  bookmarks: `{bookmarks % '{bookmark}${ESCAPED_NULL_CHAR}'}`,
  remoteBookmarks: `{remotenames % '{remotename}${ESCAPED_NULL_CHAR}'}`,
  // We use `{p1node|short} {p2node|short}` instead of `{parents}`
  // because `{parents}` only prints when a node has more than one parent,
  // not when a node has one natural parent.
  // Reference: `sl help templates`
  parents: `{p1node|short}${ESCAPED_NULL_CHAR}{p2node|short}${ESCAPED_NULL_CHAR}`,
  isHead: `{ifcontains(rev, revset('.'), '${HEAD_MARKER}')}`,
  filesAdded: '{file_adds|json}',
  filesModified: '{file_mods|json}',
  filesRemoved: '{file_dels|json}',
  successorInfo: '{mutations % "{operation}:{successors % "{node|short}"},"}',
  // This would be more elegant as a new built-in template
  diffId: '{if(phabdiff, phabdiff, github_pull_request_number)}',
  stableCommitMetadata: Internal.stableCommitTemplate ?? '',
  // Description must be last
  description: '{desc}',
};

type ConflictFileData = {contents: string; exists: boolean; isexec: boolean; issymlink: boolean};
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

function fromEntries<V>(entries: Array<[string, V]>): {
  [key: string]: V;
} {
  // Object.fromEntries() is available in Node v12 and later.
  if (typeof Object.fromEntries === 'function') {
    return Object.fromEntries(entries);
  }

  const obj: {
    [key: string]: V;
  } = {};
  for (const [key, value] of entries) {
    obj[key] = value;
  }
  return obj;
}

const FIELD_INDEX = fromEntries(Object.keys(FIELDS).map((key, i) => [key, i])) as {
  [key in Required<keyof typeof FIELDS>]: number;
};

const FETCH_TEMPLATE = [...Object.values(FIELDS), COMMIT_END_MARK].join('\n');

/**
 * This class is responsible for providing information about the working copy
 * for a Sapling repository.
 *
 * A Repository may be reused by multiple connections, not just one ISL window.
 * This is so we don't duplicate watchman subscriptions and calls to status/log.
 * A Repository does not have a pre-defined `cwd`, so it may be re-used across cwds.
 *
 * Prefer using `RepositoryCache.getOrCreate()` to access and dispose `Repository`s.
 */
export class Repository {
  public IGNORE_COMMIT_MESSAGE_LINES_REGEX = /^((?:HG|SL):.*)/gm;

  private mergeConflicts: MergeConflicts | undefined = undefined;
  private uncommittedChanges: FetchedUncommittedChanges | null = null;
  private smartlogCommits: FetchedCommits | null = null;

  private mergeConflictsEmitter = new TypedEventEmitter<'change', MergeConflicts | undefined>();
  private uncommittedChangesEmitter = new TypedEventEmitter<'change', FetchedUncommittedChanges>();
  private smartlogCommitsChangesEmitter = new TypedEventEmitter<'change', FetchedCommits>();

  private smartlogCommitsBeginFetchingEmitter = new TypedEventEmitter<'start', undefined>();
  private uncommittedChangesBeginFetchingEmitter = new TypedEventEmitter<'start', undefined>();

  private disposables: Array<() => void> = [
    () => this.mergeConflictsEmitter.removeAllListeners(),
    () => this.uncommittedChangesEmitter.removeAllListeners(),
    () => this.smartlogCommitsChangesEmitter.removeAllListeners(),
    () => this.smartlogCommitsBeginFetchingEmitter.removeAllListeners(),
    () => this.uncommittedChangesBeginFetchingEmitter.removeAllListeners(),
  ];
  public onDidDispose(callback: () => unknown): void {
    this.disposables.push(callback);
  }

  private operationQueue: OperationQueue;
  private watchForChanges: WatchForChanges;
  private pageFocusTracker = new PageFocusTracker();
  public codeReviewProvider?: CodeReviewProvider;

  private currentVisibleCommitRangeIndex = 0;
  private visibleCommitRanges: Array<number | undefined> = [
    DEFAULT_DAYS_OF_COMMITS_TO_LOAD,
    60,
    undefined,
  ];

  /**  Prefer using `RepositoryCache.getOrCreate()` to access and dispose `Repository`s. */
  constructor(public info: ValidatedRepoInfo, public logger: Logger) {
    const remote = info.codeReviewSystem;
    if (remote.type === 'github') {
      this.codeReviewProvider = new GitHubCodeReviewProvider(remote, logger);
    }

    if (remote.type === 'phabricator' && Internal?.PhabricatorCodeReviewProvider != null) {
      this.codeReviewProvider = new Internal.PhabricatorCodeReviewProvider(remote, logger);
    }

    this.watchForChanges = new WatchForChanges(info, logger, this.pageFocusTracker, kind => {
      if (kind === 'uncommitted changes') {
        this.fetchUncommittedChanges();
      } else if (kind === 'commits') {
        this.fetchSmartlogCommits();
      } else if (kind === 'merge conflicts') {
        this.checkForMergeConflicts();
      } else if (kind === 'everything') {
        this.fetchUncommittedChanges();
        this.fetchSmartlogCommits();
        this.checkForMergeConflicts();

        this.codeReviewProvider?.triggerDiffSummariesFetch(
          // We could choose to only fetch the diffs that changed (`newDiffs`) rather than all diffs,
          // but our UI doesn't cache old values, thus all other diffs would appear empty
          this.getAllDiffIds(),
        );
      }
    });

    this.operationQueue = new OperationQueue(
      this.logger,
      (
        operation: RunnableOperation,
        cwd: string,
        handleCommandProgress,
        signal: AbortSignal,
      ): Promise<void> => {
        if (operation.runner === CommandRunner.Sapling) {
          return this.runOperation(operation, handleCommandProgress, cwd, signal);
        } else if (operation.runner === CommandRunner.CodeReviewProvider) {
          const normalizedArgs = this.normalizeOperationArgs(cwd, operation.args);
          if (this.codeReviewProvider?.runExternalCommand == null) {
            return Promise.reject(
              Error('CodeReviewProvider does not support running external commands'),
            );
          }

          return (
            this.codeReviewProvider?.runExternalCommand(
              cwd,
              normalizedArgs,
              handleCommandProgress,
              signal,
            ) ?? Promise.resolve()
          );
        }
        return Promise.resolve();
      },
    );

    // refetch summaries whenever we see new diffIds
    const seenDiffs = new Set();
    const subscription = this.subscribeToSmartlogCommitsChanges(fetched => {
      if (fetched.commits.value) {
        const newDiffs = [];
        const diffIds = fetched.commits.value
          .filter(commit => commit.diffId != null)
          .map(commit => commit.diffId);
        for (const diffId of diffIds) {
          if (!seenDiffs.has(diffId)) {
            newDiffs.push(diffId);
            seenDiffs.add(diffId);
          }
        }
        if (newDiffs.length > 0) {
          this.codeReviewProvider?.triggerDiffSummariesFetch(
            // We could choose to only fetch the diffs that changed (`newDiffs`) rather than all diffs,
            // but our UI doesn't cache old values, thus all other diffs would appear empty
            this.getAllDiffIds(),
          );
        }
      }
    });

    // the repo may already be in a conflict state on startup
    this.checkForMergeConflicts();

    this.disposables.push(() => subscription.dispose());
  }

  public nextVisibleCommitRangeInDays(): number | undefined {
    if (this.currentVisibleCommitRangeIndex + 1 < this.visibleCommitRanges.length) {
      this.currentVisibleCommitRangeIndex++;
    }
    return this.visibleCommitRanges[this.currentVisibleCommitRangeIndex];
  }

  /**
   * Typically, disposing is handled by `RepositoryCache` and not used directly.
   */
  public dispose() {
    this.disposables.forEach(dispose => dispose());
    this.codeReviewProvider?.dispose();
    this.watchForChanges.dispose();
  }

  public onChangeConflictState(
    callback: (conflicts: MergeConflicts | undefined) => unknown,
  ): Disposable {
    this.mergeConflictsEmitter.on('change', callback);

    if (this.mergeConflicts) {
      // if we're already in merge conflicts, let the client know right away
      callback(this.mergeConflicts);
    }

    return {dispose: () => this.mergeConflictsEmitter.off('change', callback)};
  }

  public checkForMergeConflicts = serializeAsyncCall(async () => {
    this.logger.info('checking for merge conflicts');
    // Fast path: check if .sl/merge dir changed
    const wasAlreadyInConflicts = this.mergeConflicts != null;
    if (!wasAlreadyInConflicts) {
      const mergeDirExists = await exists(path.join(this.info.dotdir, 'merge'));
      if (!mergeDirExists) {
        // Not in a conflict
        this.logger.info(
          `conflict state still the same (${
            wasAlreadyInConflicts ? 'IN merge conflict' : 'NOT in conflict'
          })`,
        );
        return;
      }
    }

    if (this.mergeConflicts == null) {
      // notify UI that merge conflicts were detected and full details are loading
      this.mergeConflicts = {state: 'loading'};
      this.mergeConflictsEmitter.emit('change', this.mergeConflicts);
    }

    // More expensive full check for conflicts. Necessary if we see .sl/merge change, or if
    // we're already in a conflict and need to re-check if a conflict was resolved.

    let output: ResolveCommandConflictOutput;
    const fetchStartTimestamp = Date.now();
    try {
      // TODO: is this command fast on large files? it includes full conflicting file contents!
      // `sl resolve --list --all` does not seem to give any way to disambiguate (all conflicts resolved) and (not in merge)
      const proc = await this.runCommand(['resolve', '--tool', 'internal:dumpjson', '--all']);
      output = JSON.parse(proc.stdout) as ResolveCommandConflictOutput;
    } catch (err) {
      this.logger.error(`failed to check for merge conflicts: ${err}`);
      // To avoid being stuck in "loading" state forever, let's pretend there's no conflicts.
      this.mergeConflicts = undefined;
      this.mergeConflictsEmitter.emit('change', this.mergeConflicts);
      return;
    }

    this.mergeConflicts = computeNewConflicts(this.mergeConflicts, output, fetchStartTimestamp);
    this.logger.info(`repo ${this.mergeConflicts ? 'IS' : 'IS NOT'} in merge conflicts`);
    if (this.mergeConflicts) {
      const maxConflictsToLog = 20;
      const remainingConflicts = (this.mergeConflicts.files ?? [])
        .filter(conflict => conflict.status === 'U')
        .map(conflict => conflict.path)
        .slice(0, maxConflictsToLog);
      this.logger.info('remaining files with conflicts: ', remainingConflicts);
    }
    this.mergeConflictsEmitter.emit('change', this.mergeConflicts);
  });

  public getMergeConflicts(): MergeConflicts | undefined {
    return this.mergeConflicts;
  }

  /**
   * Determine basic repo info including the root and important config values.
   * Resulting RepoInfo may have null fields if cwd is not a valid repo root.
   * Throws if `command` is not found.
   */
  static async getRepoInfo(command: string, logger: Logger, cwd: string): Promise<RepoInfo> {
    const [repoRoot, dotdir, pathsDefault, pullRequestDomain, preferredSubmitCommand] =
      await Promise.all([
        findRoot(command, logger, cwd).catch((err: Error) => err),
        findDotDir(command, logger, cwd),
        getConfig(command, logger, cwd, 'paths.default').then(value => value ?? ''),
        getConfig(command, logger, cwd, 'github.pull_request_domain'),
        getConfig(command, logger, cwd, 'github.preferred_submit_command').then(
          value => value || undefined,
        ),
      ]);
    if (repoRoot instanceof Error) {
      return {type: 'invalidCommand', command};
    }
    if (repoRoot == null || dotdir == null) {
      return {type: 'cwdNotARepository', cwd};
    }

    let codeReviewSystem: CodeReviewSystem;
    const isMononoke = /^(mononoke|fb):\/\/.*/.test(pathsDefault);
    if (isMononoke) {
      // TODO: where should we be getting this from? arcconfig instead? do we need this?
      const repo = pathsDefault.slice(pathsDefault.lastIndexOf('/') + 1);
      codeReviewSystem = {type: 'phabricator', repo};
    } else if (pathsDefault === '') {
      codeReviewSystem = {type: 'none'};
    } else {
      const repoInfo = extractRepoInfoFromUrl(pathsDefault);
      if (
        repoInfo != null &&
        (repoInfo.hostname === 'github.com' || (await isGithubEnterprise(repoInfo.hostname)))
      ) {
        const {owner, repo, hostname} = repoInfo;
        codeReviewSystem = {
          type: 'github',
          owner,
          repo,
          hostname,
        };
      } else {
        codeReviewSystem = {type: 'unknown', path: pathsDefault};
      }
    }

    const result: RepoInfo = {
      type: 'success',
      command,
      dotdir,
      repoRoot,
      codeReviewSystem,
      pullRequestDomain,
      preferredSubmitCommand: preferredSubmitCommand as PreferredSubmitCommand | undefined,
    };
    logger.info('repo info: ', result);
    return result;
  }

  /**
   * Run long-lived command which mutates the repository state.
   * Progress is streamed back as it comes in.
   * Operations are run immediately. For queueing, see OperationQueue.
   */
  async runOrQueueOperation(
    operation: RunnableOperation,
    onProgress: (progress: OperationProgress) => void,
    tracker: ServerSideTracker,
    cwd: string,
  ): Promise<void> {
    await this.operationQueue.runOrQueueOperation(operation, onProgress, tracker, cwd);

    // After any operation finishes, make sure we poll right away,
    // so the UI is guarnateed to get the latest data.
    this.watchForChanges.poll('force');
  }

  /**
   * Abort the running operation if it matches the given id.
   */
  abortRunningOpeation(operationId: string) {
    this.operationQueue.abortRunningOperation(operationId);
  }

  private normalizeOperationArgs(cwd: string, args: Array<CommandArg>): Array<string> {
    const repoRoot = unwrap(this.info.repoRoot);
    return args.map(arg => {
      if (typeof arg === 'object') {
        switch (arg.type) {
          case 'repo-relative-file':
            return path.normalize(path.relative(cwd, path.join(repoRoot, arg.path)));
          case 'succeedable-revset':
            return `max(successors(${arg.revset}))`;
        }
      }
      return arg;
    });
  }

  /**
   * Called by this.operationQueue in response to runOrQueueOperation when an operation is ready to actually run.
   */
  private async runOperation(
    operation: {
      id: string;
      args: Array<CommandArg>;
    },
    onProgress: OperationCommandProgressReporter,
    cwd: string,
    signal: AbortSignal,
  ): Promise<void> {
    const cwdRelativeArgs = this.normalizeOperationArgs(cwd, operation.args);
    const {command, args, options} = getExecParams(this.info.command, cwdRelativeArgs, cwd);

    this.logger.log('run operation: ', command, cwdRelativeArgs.join(' '));

    const execution = execa(command, args, {...options, stdout: 'pipe', stderr: 'pipe'});
    // It would be more appropriate to call this in reponse to execution.on('spawn'), but
    // this seems to be inconsistent about firing in all versions of node.
    // Just send spawn immediately. Errors during spawn like ENOENT will still be reported by `exit`.
    onProgress('spawn');
    execution.stdout?.on('data', data => {
      onProgress('stdout', data.toString());
    });
    execution.stderr?.on('data', data => {
      onProgress('stderr', data.toString());
    });
    execution.on('exit', exitCode => {
      onProgress('exit', exitCode || 0);
    });
    signal.addEventListener('abort', () => {
      this.logger.log('kill operation: ', command, cwdRelativeArgs.join(' '));
    });
    handleAbortSignalOnProcess(execution, signal);
    await execution;
  }

  setPageFocus(page: string, state: PageVisibility) {
    this.pageFocusTracker.setState(page, state);
  }

  /** Return the latest fetched value for UncommittedChanges. */
  getUncommittedChanges(): FetchedUncommittedChanges | null {
    return this.uncommittedChanges;
  }

  subscribeToUncommittedChanges(
    callback: (result: FetchedUncommittedChanges) => unknown,
  ): Disposable {
    this.uncommittedChangesEmitter.on('change', callback);
    return {
      dispose: () => {
        this.uncommittedChangesEmitter.off('change', callback);
      },
    };
  }

  fetchUncommittedChanges = serializeAsyncCall(async () => {
    const fetchStartTimestamp = Date.now();
    try {
      this.uncommittedChangesBeginFetchingEmitter.emit('start');
      // Note `status -tjson` run with PLAIN are repo-relative
      const proc = await this.runCommand(['status', '-Tjson', '--copies']);
      const files = (JSON.parse(proc.stdout) as UncommittedChanges).map(change => ({
        ...change,
        path: removeLeadingPathSep(change.path),
      }));

      this.uncommittedChanges = {
        fetchStartTimestamp,
        fetchCompletedTimestamp: Date.now(),
        files: {value: files},
      };
      this.uncommittedChangesEmitter.emit('change', this.uncommittedChanges);
    } catch (err) {
      this.logger.error('Error fetching files: ', err);
      if (isProcessError(err)) {
        if (err.stderr.includes('checkout is currently in progress')) {
          this.logger.info('Ignoring `hg status` error caused by in-progress checkout');
          return;
        }
      }
      // emit an error, but don't save it to this.uncommittedChanges
      this.uncommittedChangesEmitter.emit('change', {
        fetchStartTimestamp,
        fetchCompletedTimestamp: Date.now(),
        files: {error: err instanceof Error ? err : new Error(err as string)},
      });
    }
  });

  /** Return the latest fetched value for SmartlogCommits. */
  getSmartlogCommits(): FetchedCommits | null {
    return this.smartlogCommits;
  }

  subscribeToSmartlogCommitsChanges(callback: (result: FetchedCommits) => unknown) {
    this.smartlogCommitsChangesEmitter.on('change', callback);
    return {
      dispose: () => {
        this.smartlogCommitsChangesEmitter.off('change', callback);
      },
    };
  }

  subscribeToSmartlogCommitsBeginFetching(callback: (isFetching: boolean) => unknown) {
    const onStart = () => callback(true);
    this.smartlogCommitsBeginFetchingEmitter.on('start', onStart);
    return {
      dispose: () => {
        this.smartlogCommitsBeginFetchingEmitter.off('start', onStart);
      },
    };
  }

  subscribeToUncommittedChangesBeginFetching(callback: (isFetching: boolean) => unknown) {
    const onStart = () => callback(true);
    this.uncommittedChangesBeginFetchingEmitter.on('start', onStart);
    return {
      dispose: () => {
        this.uncommittedChangesBeginFetchingEmitter.off('start', onStart);
      },
    };
  }

  fetchSmartlogCommits = serializeAsyncCall(async () => {
    const fetchStartTimestamp = Date.now();
    try {
      this.smartlogCommitsBeginFetchingEmitter.emit('start');
      const visibleCommitDayRange = this.visibleCommitRanges[this.currentVisibleCommitRangeIndex];
      const revset = !visibleCommitDayRange
        ? 'smartlog()'
        : // filter default smartlog query by date range
          `smartlog(((interestingbookmarks() + heads(draft())) & date(-${visibleCommitDayRange})) + .)`;
      const proc = await this.runCommand(['log', '--template', FETCH_TEMPLATE, '--rev', revset]);
      const commits = parseCommitInfoOutput(this.logger, proc.stdout.trim());
      if (commits.length === 0) {
        throw new Error(ErrorShortMessages.NoCommitsFetched);
      }
      this.smartlogCommits = {
        fetchStartTimestamp,
        fetchCompletedTimestamp: Date.now(),
        commits: {value: commits},
      };
      this.smartlogCommitsChangesEmitter.emit('change', this.smartlogCommits);
    } catch (err) {
      this.logger.error('Error fetching commits: ', err);
      this.smartlogCommitsChangesEmitter.emit('change', {
        fetchStartTimestamp,
        fetchCompletedTimestamp: Date.now(),
        commits: {error: err instanceof Error ? err : new Error(err as string)},
      });
    }
  });

  /** Watch for changes to the head commit, e.g. from checking out a new commit */
  subscribeToHeadCommit(callback: (head: CommitInfo) => unknown) {
    let headCommit = this.smartlogCommits?.commits.value?.find(commit => commit.isHead);
    if (headCommit != null) {
      callback(headCommit);
    }
    const onData = (data: FetchedCommits) => {
      const newHead = data?.commits.value?.find(commit => commit.isHead);
      if (newHead != null && newHead.hash !== headCommit?.hash) {
        callback(newHead);
        headCommit = newHead;
      }
    };
    this.smartlogCommitsChangesEmitter.on('change', onData);
    return {
      dispose: () => {
        this.smartlogCommitsChangesEmitter.off('change', onData);
      },
    };
  }

  private catLimiter = new RateLimiter(MAX_SIMULTANEOUS_CAT_CALLS, s =>
    this.logger.info('[cat]', s),
  );
  /** Return file content at a given revset, e.g. hash or `.` */
  public cat(file: AbsolutePath, rev: Revset): Promise<string> {
    return this.catLimiter.enqueueRun(async () => {
      // For `sl cat`, we want the output of the command verbatim.
      const options = {stripFinalNewline: false};
      return (await this.runCommand(['cat', file, '--rev', rev], /*cwd=*/ undefined, options))
        .stdout;
    });
  }

  public getAllDiffIds(): Array<DiffId> {
    return (
      this.getSmartlogCommits()
        ?.commits.value?.map(commit => commit.diffId)
        .filter(notEmpty) ?? []
    );
  }

  public runCommand(args: Array<string>, cwd?: string, options?: execa.Options) {
    return runCommand(
      this.info.command,
      args,
      this.logger,
      unwrap(cwd ?? this.info.repoRoot),
      options,
    );
  }

  public getConfig(configName: string): Promise<string | undefined> {
    return getConfig(this.info.command, this.logger, this.info.repoRoot, configName);
  }
  public setConfig(level: ConfigLevel, configName: string, configValue: string): Promise<void> {
    return setConfig(
      this.info.command,
      this.logger,
      this.info.repoRoot,
      level,
      configName,
      configValue,
    );
  }
}

function runCommand(
  command_: string,
  args_: Array<string>,
  logger: Logger,
  cwd: string,
  options_?: execa.Options,
): execa.ExecaChildProcess {
  const {command, args, options} = getExecParams(command_, args_, cwd, options_);
  logger.log('run command: ', command, args[0]);
  return execa(command, args, options);
}

export const __TEST__ = {
  runCommand,
};

/**
 * Root of the repository where the .sl folder lives.
 * Throws only if `command` is invalid, so this check can double as validation of the `sl` command */
async function findRoot(
  command: string,
  logger: Logger,
  cwd: string,
): Promise<AbsolutePath | undefined> {
  try {
    return (await runCommand(command, ['root'], logger, cwd)).stdout;
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
      logger.error(`command ${command} not found`, error);
      throw error;
    }
  }
}

async function findDotDir(
  command: string,
  logger: Logger,
  cwd: string,
): Promise<AbsolutePath | undefined> {
  try {
    return (await runCommand(command, ['root', '--dotdir'], logger, cwd)).stdout;
  } catch (error) {
    logger.error(`Failed to find repository dotdir in ${cwd}`, error);
    return undefined;
  }
}

async function getConfig(
  command: string,
  logger: Logger,
  cwd: string,
  configName: string,
): Promise<string | undefined> {
  try {
    return (await runCommand(command, ['config', configName], logger, cwd)).stdout.trim();
  } catch {
    // `config` exits with status 1 if config is not set. This is not an error.
    return undefined;
  }
}
type ConfigLevel = 'user' | 'system' | 'local';
async function setConfig(
  command: string,
  logger: Logger,
  cwd: string,
  level: ConfigLevel,
  configName: string,
  configValue: string,
): Promise<void> {
  await runCommand(command, ['config', `--${level}`, configName, configValue], logger, cwd);
}

function getExecParams(
  command: string,
  args_: Array<string>,
  cwd: string,
  options_?: execa.Options,
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
  const options: execa.Options = {
    ...options_,
    env: {
      LANG: 'en_US.utf-8', // make sure to use unicode if user hasn't set LANG themselves
      // TODO: remove when SL_ENCODING is used everywhere
      HGENCODING: 'UTF-8',
      SL_ENCODING: 'UTF-8',
      // override any custom aliases a user has defined.
      SL_AUTOMATION: 'true',
      // Prevent user-specified merge tools from attempting to
      // open interactive editors.
      HGMERGE: ':merge3',
      SL_MERGE: ':merge3',
      EDITOR: undefined,
    } as unknown as NodeJS.ProcessEnv,
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
 * Extract CommitInfos from log calls that use FETCH_TEMPLATE.
 */
export function parseCommitInfoOutput(logger: Logger, output: string): SmartlogCommits {
  const revisions = output.split(COMMIT_END_MARK);
  const commitInfos: Array<CommitInfo> = [];
  for (const chunk of revisions) {
    try {
      const lines = chunk.trim().split('\n');
      if (lines.length < Object.keys(FIELDS).length) {
        continue;
      }
      const files: Array<ChangedFile> = [
        ...(JSON.parse(lines[FIELD_INDEX.filesModified]) as Array<string>).map(path => ({
          path,
          status: 'M' as const,
        })),
        ...(JSON.parse(lines[FIELD_INDEX.filesAdded]) as Array<string>).map(path => ({
          path,
          status: 'A' as const,
        })),
        ...(JSON.parse(lines[FIELD_INDEX.filesRemoved]) as Array<string>).map(path => ({
          path,
          status: 'R' as const,
        })),
      ];
      commitInfos.push({
        hash: lines[FIELD_INDEX.hash],
        title: lines[FIELD_INDEX.title],
        author: lines[FIELD_INDEX.author],
        date: new Date(lines[FIELD_INDEX.date]),
        parents: splitLine(lines[FIELD_INDEX.parents]).filter(hash => hash !== NO_NODE_HASH) as [
          string,
          string,
        ],
        phase: lines[FIELD_INDEX.phase] as CommitPhaseType,
        bookmarks: splitLine(lines[FIELD_INDEX.bookmarks]),
        remoteBookmarks: splitLine(lines[FIELD_INDEX.remoteBookmarks]),
        isHead: lines[FIELD_INDEX.isHead] === HEAD_MARKER,
        filesSample: files.slice(0, MAX_FETCHED_FILES_PER_COMMIT),
        totalFileCount: files.length,
        successorInfo: parseSuccessorData(lines[FIELD_INDEX.successorInfo]),
        description: lines
          .slice(FIELD_INDEX.description + 1 /* first field of description is title; skip it */)
          .join('\n'),
        diffId: lines[FIELD_INDEX.diffId] != '' ? lines[FIELD_INDEX.diffId] : undefined,
        stableCommitMetadata:
          lines[FIELD_INDEX.stableCommitMetadata] != ''
            ? lines[FIELD_INDEX.stableCommitMetadata]
            : undefined,
      });
    } catch (err) {
      logger.error('failed to parse commit');
    }
  }
  return commitInfos;
}
export function parseSuccessorData(successorData: string): SuccessorInfo | undefined {
  const [successorString] = successorData.split(',', 1); // we're only interested in the first available mutation
  if (!successorString) {
    return undefined;
  }
  const successor = successorString.split(':');
  return {
    hash: successor[1],
    type: successor[0],
  };
}
function splitLine(line: string): Array<string> {
  return line.split(NULL_CHAR).filter(e => e.length > 0);
}

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

/**
 * Returns absolute path for a repo-relative file path.
 * If the path "escapes" the repository's root dir, returns null
 * Used to validate that a file path does not "escape" the repo, and the file can safely be modified on the filesystem.
 * absolutePathForFileInRepo("foo/bar/file.txt", repo) -> /path/to/repo/foo/bar/file.txt
 * absolutePathForFileInRepo("../file.txt", repo) -> null
 */
export function absolutePathForFileInRepo(
  filePath: RepoRelativePath,
  repo: Repository,
  pathMod = path,
): AbsolutePath | null {
  // Note that resolve() is contractually obligated to return an absolute path.
  const fullPath = pathMod.resolve(repo.info.repoRoot, filePath);
  // Prefix checks on paths can be footguns on Windows for C:\\ vs c:\\, but since
  // we use the same exact path check here and in the resolve, there should be
  // no incompatibility here.
  if (fullPath.startsWith(repo.info.repoRoot + pathMod.sep)) {
    return fullPath;
  } else {
    return null;
  }
}

export function repoRelativePathForAbsolutePath(
  absolutePath: AbsolutePath,
  repo: Repository,
  pathMod = path,
): RepoRelativePath {
  return pathMod.relative(repo.info.repoRoot, absolutePath);
}

function isProcessError(s: unknown): s is {stderr: string} {
  return s != null && typeof s === 'object' && 'stderr' in s;
}

function computeNewConflicts(
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
