/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CodeReviewProvider} from './CodeReviewProvider';
import type {KindOfChange, PollKind} from './WatchForChanges';
import type {TrackEventName} from './analytics/eventNames';
import type {ConfigLevel, ResolveCommandConflictOutput} from './commands';
import type {RepositoryContext} from './serverTypes';
import type {
  CommitInfo,
  Disposable,
  CommandArg,
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
  FetchedUncommittedChanges,
  FetchedCommits,
  ShelvedChange,
  CommitCloudSyncState,
  Hash,
  ConfigName,
  Alert,
  RepoRelativePath,
  SettableConfigName,
  StableInfo,
} from 'isl/src/types';
import type {Comparison} from 'shared/Comparison';

import {Internal} from './Internal';
import {OperationQueue} from './OperationQueue';
import {PageFocusTracker} from './PageFocusTracker';
import {WatchForChanges} from './WatchForChanges';
import {parseAlerts} from './alerts';
import {
  MAX_SIMULTANEOUS_CAT_CALLS,
  READ_COMMAND_TIMEOUT_MS,
  computeNewConflicts,
  extractRepoInfoFromUrl,
  findDotDir,
  findRoot,
  getConfigs,
  getExecParams,
  runCommand,
  setConfig,
} from './commands';
import {DEFAULT_DAYS_OF_COMMITS_TO_LOAD, ErrorShortMessages} from './constants';
import {GitHubCodeReviewProvider} from './github/githubCodeReviewProvider';
import {isGithubEnterprise} from './github/queryGraphQL';
import {
  CHANGED_FILES_FIELDS,
  CHANGED_FILES_INDEX,
  CHANGED_FILES_TEMPLATE,
  COMMIT_END_MARK,
  FETCH_TEMPLATE,
  SHELVE_FETCH_TEMPLATE,
  attachStableLocations,
  parseCommitInfoOutput,
  parseShelvedCommitsOutput,
} from './templates';
import {handleAbortSignalOnProcess, isExecaError, serializeAsyncCall} from './utils';
import execa from 'execa';
import {
  settableConfigNames,
  allConfigNames,
  CommitCloudBackupStatus,
  CommandRunner,
} from 'isl/src/types';
import path from 'path';
import {revsetArgsForComparison} from 'shared/Comparison';
import {LRU} from 'shared/LRU';
import {RateLimiter} from 'shared/RateLimiter';
import {TypedEventEmitter} from 'shared/TypedEventEmitter';
import {exists} from 'shared/fs';
import {removeLeadingPathSep} from 'shared/pathUtils';
import {notEmpty, randomId, nullthrows} from 'shared/utils';

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
  public IGNORE_COMMIT_MESSAGE_LINES_REGEX = /^((?:HG|SL):.*)\n?/gm;

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

  /**
   * Config: milliseconds to hold off log/status refresh during the start of a command.
   * This is to avoid showing messy indeterminate states (like millions of files changed
   * during a long distance checkout, or commit graph changed but '.' is out of sync).
   *
   * Default: 10 seconds. Can be set by the `isl.hold-off-refresh-ms` setting.
   */
  public configHoldOffRefreshMs = 10000;

  private configRateLimiter = new RateLimiter(1);

  private currentVisibleCommitRangeIndex = 0;
  private visibleCommitRanges: Array<number | undefined> = [
    DEFAULT_DAYS_OF_COMMITS_TO_LOAD,
    60,
    undefined,
  ];

  /**
   * Additional commits to include in batched `log` fetch,
   * used for additional remote bookmarks / known stable commit hashes.
   * After fetching commits, stable names will be added to commits in "stableCommitMetadata"
   */
  public stableLocations: Array<StableInfo> = [];

  /**
   * The context used when the repository was created.
   * This is needed for subscriptions to have access to ANY logger, etc.
   * Avoid using this, and prefer using the correct context for a given connection.
   */
  public initialConnectionContext: RepositoryContext;

  /**  Prefer using `RepositoryCache.getOrCreate()` to access and dispose `Repository`s. */
  constructor(public info: ValidatedRepoInfo, ctx: RepositoryContext) {
    this.initialConnectionContext = ctx;

    const remote = info.codeReviewSystem;
    if (remote.type === 'github') {
      this.codeReviewProvider = new GitHubCodeReviewProvider(remote, ctx.logger);
    }

    if (remote.type === 'phabricator' && Internal?.PhabricatorCodeReviewProvider != null) {
      this.codeReviewProvider = new Internal.PhabricatorCodeReviewProvider(
        remote,
        this.initialConnectionContext,
      );
    }

    const shouldWait = (): boolean => {
      const startTime = this.operationQueue.getRunningOperationStartTime();
      if (startTime == null) {
        return false;
      }
      // Prevent auto-refresh during the first 10 seconds of a running command.
      // When a command is running, the intermediate state can be messy:
      // - status errors out (edenfs), is noisy (long distance goto)
      // - commit graph and the `.` are updated separately and hard to predict
      // Let's just rely on optimistic state to provide the "clean" outcome.
      // In case the command takes a long time to run, allow refresh after
      // the time period.
      // Fundementally, the intermediate states have no choice but have to
      // be messy because filesystems are not transactional (and reading in
      // `sl` is designed to be lock-free).
      const elapsedMs = Date.now() - startTime.valueOf();
      const result = elapsedMs < this.configHoldOffRefreshMs;
      return result;
    };
    const callback = (kind: KindOfChange, pollKind?: PollKind) => {
      if (pollKind !== 'force' && shouldWait()) {
        // Do nothing. This is fine because after the operation
        // there will be a refresh.
        ctx.logger.info('polling prevented from shouldWait');
        return;
      }
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
    };
    this.watchForChanges = new WatchForChanges(info, ctx.logger, this.pageFocusTracker, callback);

    this.operationQueue = new OperationQueue(
      (
        ctx: RepositoryContext,
        operation: RunnableOperation,
        handleCommandProgress,
        signal: AbortSignal,
      ): Promise<void> => {
        const {cwd} = ctx;
        if (operation.runner === CommandRunner.Sapling) {
          return this.runOperation(ctx, operation, handleCommandProgress, signal);
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
        } else if (operation.runner === CommandRunner.InternalArcanist) {
          const normalizedArgs = this.normalizeOperationArgs(cwd, operation.args);
          return (
            void Internal.runArcanistCommand?.(
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

    this.applyConfigInBackground(ctx);
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
    this.initialConnectionContext.logger.info('checking for merge conflicts');
    // Fast path: check if .sl/merge dir changed
    const wasAlreadyInConflicts = this.mergeConflicts != null;
    if (!wasAlreadyInConflicts) {
      const mergeDirExists = await exists(path.join(this.info.dotdir, 'merge'));
      if (!mergeDirExists) {
        // Not in a conflict
        this.initialConnectionContext.logger.info(
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
      const proc = await this.runCommand(
        ['resolve', '--tool', 'internal:dumpjson', '--all'],
        'GetConflictsCommand',
        this.initialConnectionContext,
      );
      output = JSON.parse(proc.stdout) as ResolveCommandConflictOutput;
    } catch (err) {
      this.initialConnectionContext.logger.error(`failed to check for merge conflicts: ${err}`);
      // To avoid being stuck in "loading" state forever, let's pretend there's no conflicts.
      this.mergeConflicts = undefined;
      this.mergeConflictsEmitter.emit('change', this.mergeConflicts);
      return;
    }

    this.mergeConflicts = computeNewConflicts(this.mergeConflicts, output, fetchStartTimestamp);
    this.initialConnectionContext.logger.info(
      `repo ${this.mergeConflicts ? 'IS' : 'IS NOT'} in merge conflicts`,
    );
    if (this.mergeConflicts) {
      const maxConflictsToLog = 20;
      const remainingConflicts = (this.mergeConflicts.files ?? [])
        .filter(conflict => conflict.status === 'U')
        .map(conflict => conflict.path)
        .slice(0, maxConflictsToLog);
      this.initialConnectionContext.logger.info(
        'remaining files with conflicts: ',
        remainingConflicts,
      );
    }
    this.mergeConflictsEmitter.emit('change', this.mergeConflicts);

    if (!wasAlreadyInConflicts && this.mergeConflicts) {
      this.initialConnectionContext.tracker.track('EnterMergeConflicts', {
        extras: {numConflicts: this.mergeConflicts.files?.length ?? 0},
      });
    } else if (wasAlreadyInConflicts && !this.mergeConflicts) {
      this.initialConnectionContext.tracker.track('ExitMergeConflicts', {extras: {}});
    }
  });

  public getMergeConflicts(): MergeConflicts | undefined {
    return this.mergeConflicts;
  }

  /**
   * Determine basic repo info including the root and important config values.
   * Resulting RepoInfo may have null fields if cwd is not a valid repo root.
   * Throws if `command` is not found.
   */
  static async getRepoInfo(ctx: RepositoryContext): Promise<RepoInfo> {
    const {cmd, cwd, logger} = ctx;
    const [repoRoot, dotdir, configs] = await Promise.all([
      findRoot(ctx).catch((err: Error) => err),
      findDotDir(ctx),
      // TODO: This should actually use expanded paths, since the config won't handle custom schemes.
      // However, `sl debugexpandpaths` is currently too slow and impacts startup time.
      getConfigs(ctx, [
        'paths.default',
        'github.pull_request_domain',
        'github.preferred_submit_command',
      ]),
    ]);
    const pathsDefault = configs.get('paths.default') ?? '';
    const pullRequestDomain = configs.get('github.pull_request_domain');
    const preferredSubmitCommand = configs.get('github.preferred_submit_command');

    if (repoRoot instanceof Error) {
      // first check that the cwd exists
      const cwdExists = await exists(cwd);
      if (!cwdExists) {
        return {type: 'cwdDoesNotExist', cwd};
      }

      return {
        type: 'invalidCommand',
        command: cmd,
        path: process.env.PATH,
      };
    }
    if (repoRoot == null || dotdir == null) {
      return {type: 'cwdNotARepository', cwd};
    }

    let codeReviewSystem: CodeReviewSystem;
    if (Internal.isMononokePath?.(pathsDefault)) {
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
      command: cmd,
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
   * This promise resolves when the operation exits.
   */
  async runOrQueueOperation(
    ctx: RepositoryContext,
    operation: RunnableOperation,
    onProgress: (progress: OperationProgress) => void,
  ): Promise<void> {
    const result = await this.operationQueue.runOrQueueOperation(ctx, operation, onProgress);

    if (result !== 'skipped') {
      // After any operation finishes, make sure we poll right away,
      // so the UI is guarnateed to get the latest data.
      this.watchForChanges.poll('force');
    }
  }

  /**
   * Abort the running operation if it matches the given id.
   */
  abortRunningOpeation(operationId: string) {
    this.operationQueue.abortRunningOperation(operationId);
  }

  /** The currently running operation tracked by the server. */
  getRunningOperation() {
    return this.operationQueue.getRunningOperation();
  }

  private normalizeOperationArgs(cwd: string, args: Array<CommandArg>): Array<string> {
    const repoRoot = nullthrows(this.info.repoRoot);
    const illegalArgs = new Set(['--cwd', '--config', '--insecure', '--repository', '-R']);
    return args.flatMap(arg => {
      if (typeof arg === 'object') {
        switch (arg.type) {
          case 'config':
            if (!(settableConfigNames as ReadonlyArray<string>).includes(arg.key)) {
              throw new Error(`config ${arg.key} not allowed`);
            }
            return ['--config', `${arg.key}=${arg.value}`];
          case 'repo-relative-file':
            return [path.normalize(path.relative(cwd, path.join(repoRoot, arg.path)))];
          case 'exact-revset':
            if (arg.revset.startsWith('-')) {
              // don't allow revsets to be used as flags
              throw new Error('invalid revset');
            }
            return [arg.revset];
          case 'succeedable-revset':
            return [`max(successors(${arg.revset}))`];
        }
      }
      if (illegalArgs.has(arg)) {
        throw new Error(`argument '${arg}' is not allowed`);
      }
      return arg;
    });
  }

  /**
   * Called by this.operationQueue in response to runOrQueueOperation when an operation is ready to actually run.
   */
  private async runOperation(
    ctx: RepositoryContext,
    operation: RunnableOperation,
    onProgress: OperationCommandProgressReporter,
    signal: AbortSignal,
  ): Promise<void> {
    const {cwd} = ctx;
    const cwdRelativeArgs = this.normalizeOperationArgs(cwd, operation.args);
    const {stdin} = operation;
    const {command, args, options} = getExecParams(
      this.info.command,
      cwdRelativeArgs,
      cwd,
      stdin ? {input: stdin} : undefined,
      Internal.additionalEnvForCommand?.(operation),
    );

    ctx.logger.log('run operation: ', command, cwdRelativeArgs.join(' '));

    const commandBlocklist = new Set(['debugshell', 'dbsh', 'debugsh']);
    if (args.some(arg => commandBlocklist.has(arg))) {
      throw new Error(`command "${args.join(' ')}" is not allowed`);
    }

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
      ctx.logger.log('kill operation: ', command, cwdRelativeArgs.join(' '));
    });
    handleAbortSignalOnProcess(execution, signal);
    await execution;
  }

  setPageFocus(page: string, state: PageVisibility) {
    this.pageFocusTracker.setState(page, state);
    this.initialConnectionContext.tracker.track('FocusChanged', {extras: {state}});
  }

  private refcount = 0;
  ref() {
    this.refcount++;
    if (this.refcount === 1) {
      this.watchForChanges.setupWatchmanSubscriptions();
    }
  }
  unref() {
    this.refcount--;
    if (this.refcount === 0) {
      this.watchForChanges.disposeWatchmanSubscriptions();
    }
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
      const proc = await this.runCommand(
        ['status', '-Tjson', '--copies'],
        'StatusCommand',
        this.initialConnectionContext,
      );
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
      this.initialConnectionContext.logger.error('Error fetching files: ', err);
      if (isExecaError(err)) {
        if (err.stderr.includes('checkout is currently in progress')) {
          this.initialConnectionContext.logger.info(
            'Ignoring `hg status` error caused by in-progress checkout',
          );
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

      const primaryRevset = '(interestingbookmarks() + heads(draft()))';

      // Revset to fetch for commits, e.g.:
      // smartlog(interestingbookmarks() + heads(draft()) + .)
      // smartlog((interestingbookmarks() + heads(draft()) & date(-14)) + .)
      // smartlog((interestingbookmarks() + heads(draft()) & date(-14)) + . + present(a1b2c3d4))
      const revset = `smartlog(${[
        !visibleCommitDayRange
          ? primaryRevset
          : // filter default smartlog query by date range
            `(${primaryRevset} & date(-${visibleCommitDayRange}))`,
        '.', // always include wdir parent
        // stable locations hashes may be newer than the repo has, wrap in `present()` to only include if available.
        ...this.stableLocations.map(location => `present(${location.hash})`),
      ]
        .filter(notEmpty)
        .join(' + ')})`;

      const proc = await this.runCommand(
        ['log', '--template', FETCH_TEMPLATE, '--rev', revset],
        'LogCommand',
        this.initialConnectionContext,
      );
      const commits = parseCommitInfoOutput(
        this.initialConnectionContext.logger,
        proc.stdout.trim(),
      );
      if (commits.length === 0) {
        throw new Error(ErrorShortMessages.NoCommitsFetched);
      }
      attachStableLocations(commits, this.stableLocations);
      this.smartlogCommits = {
        fetchStartTimestamp,
        fetchCompletedTimestamp: Date.now(),
        commits: {value: commits},
      };
      this.smartlogCommitsChangesEmitter.emit('change', this.smartlogCommits);
    } catch (err) {
      let error = err;
      const internalError = Internal.checkInternalError?.(err);
      if (internalError) {
        error = internalError;
      }
      if (isExecaError(error) && error.stderr.includes('Please check your internet connection')) {
        error = Error('Network request failed. Please check your internet connection.');
      }
      this.initialConnectionContext.logger.error('Error fetching commits: ', error);
      this.smartlogCommitsChangesEmitter.emit('change', {
        fetchStartTimestamp,
        fetchCompletedTimestamp: Date.now(),
        commits: {error: error instanceof Error ? error : new Error(error as string)},
      });
    }
  });

  /** Get the current head commit if loaded */
  getHeadCommit(): CommitInfo | undefined {
    return this.smartlogCommits?.commits.value?.find(commit => commit.isDot);
  }

  /** Watch for changes to the head commit, e.g. from checking out a new commit */
  subscribeToHeadCommit(callback: (head: CommitInfo) => unknown) {
    let headCommit = this.getHeadCommit();
    if (headCommit != null) {
      callback(headCommit);
    }
    const onData = (data: FetchedCommits) => {
      const newHead = data?.commits.value?.find(commit => commit.isDot);
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
    this.initialConnectionContext.logger.info('[cat]', s),
  );
  /** Return file content at a given revset, e.g. hash or `.` */
  public cat(ctx: RepositoryContext, file: AbsolutePath, rev: Revset): Promise<string> {
    return this.catLimiter.enqueueRun(async () => {
      // For `sl cat`, we want the output of the command verbatim.
      const options = {stripFinalNewline: false};
      return (await this.runCommand(['cat', file, '--rev', rev], 'CatCommand', ctx, options))
        .stdout;
    });
  }

  /**
   * Returns line-by-line blame information for a file at a given commit.
   * Returns the line content and commit info.
   * Note: the line will including trailing newline.
   */
  public async blame(
    ctx: RepositoryContext,
    filePath: string,
    hash: string,
  ): Promise<Array<[line: string, info: CommitInfo | undefined]>> {
    const t1 = Date.now();
    const output = await this.runCommand(
      ['blame', filePath, '-Tjson', '--change', '--rev', hash],
      'BlameCommand',
      ctx,
      undefined,
      /* don't timeout */ 0,
    );
    const blame = JSON.parse(output.stdout) as Array<{lines: Array<{line: string; node: string}>}>;
    const t2 = Date.now();

    if (blame.length === 0) {
      // no blame for file, perhaps it was added or untracked
      return [];
    }

    const hashes = new Set<string>();
    for (const line of blame[0].lines) {
      hashes.add(line.node);
    }
    // We don't get all the info we need from the  blame command, so we run `sl log` on the hashes.
    // TODO: we could make the blame command return this directly, which is probably faster.
    // TODO: We don't actually need all the fields in FETCH_TEMPLATE for blame. Reducing this template may speed it up as well.
    const commits = await this.lookupCommits(ctx, [...hashes]);
    const t3 = Date.now();
    ctx.logger.info(
      `Fetched ${commits.size} commits for blame. Blame took ${(t2 - t1) / 1000}s, commits took ${
        (t3 - t2) / 1000
      }s`,
    );
    return blame[0].lines.map(({node, line}) => [line, commits.get(node)]);
  }

  public async getCommitCloudState(ctx: RepositoryContext): Promise<CommitCloudSyncState> {
    const lastChecked = new Date();

    const [extension, backupStatuses, cloudStatus] = await Promise.allSettled([
      this.forceGetConfig(ctx, 'extensions.commitcloud'),
      this.fetchCommitCloudBackupStatuses(ctx),
      this.fetchCommitCloudStatus(ctx),
    ]);
    if (extension.status === 'fulfilled' && extension.value !== '') {
      return {
        lastChecked,
        isDisabled: true,
      };
    }

    if (backupStatuses.status === 'rejected') {
      return {
        lastChecked,
        syncError: backupStatuses.reason,
      };
    } else if (cloudStatus.status === 'rejected') {
      return {
        lastChecked,
        workspaceError: cloudStatus.reason,
      };
    }

    return {
      lastChecked,
      ...cloudStatus.value,
      commitStatuses: backupStatuses.value,
    };
  }

  private async fetchCommitCloudBackupStatuses(
    ctx: RepositoryContext,
  ): Promise<Map<Hash, CommitCloudBackupStatus>> {
    const revset = 'draft() - backedup()';
    const commitCloudBackupStatusTemplate = `{dict(
      hash="{node}",
      backingup="{backingup}",
      date="{date|isodatesec}"
      )|json}\n`;

    const output = await this.runCommand(
      ['log', '--rev', revset, '--template', commitCloudBackupStatusTemplate],
      'CommitCloudSyncBackupStatusCommand',
      ctx,
    );

    const rawObjects = output.stdout.trim().split('\n');
    const parsedObjects = rawObjects
      .map(rawObject => {
        try {
          return JSON.parse(rawObject) as {hash: Hash; backingup: 'True' | 'False'; date: string};
        } catch (err) {
          return null;
        }
      })
      .filter(notEmpty);

    const now = new Date();
    const TEN_MIN = 10 * 60 * 1000;
    const statuses = new Map<Hash, CommitCloudBackupStatus>(
      parsedObjects.map(obj => [
        obj.hash,
        obj.backingup === 'True'
          ? CommitCloudBackupStatus.InProgress
          : now.valueOf() - new Date(obj.date).valueOf() < TEN_MIN
          ? CommitCloudBackupStatus.Pending
          : CommitCloudBackupStatus.Failed,
      ]),
    );
    return statuses;
  }

  private async fetchCommitCloudStatus(ctx: RepositoryContext): Promise<{
    lastBackup: Date | undefined;
    currentWorkspace: string;
    workspaceChoices: Array<string>;
  }> {
    const [cloudStatusOutput, cloudListOutput] = await Promise.all([
      this.runCommand(['cloud', 'status'], 'CommitCloudStatusCommand', ctx),
      this.runCommand(['cloud', 'list'], 'CommitCloudListCommand', ctx),
    ]);

    const currentWorkspace =
      /Workspace: ([a-zA-Z/0-9._-]+)/.exec(cloudStatusOutput.stdout)?.[1] ?? 'default';
    const lastSyncTimeStr = /Last Sync Time: (.*)/.exec(cloudStatusOutput.stdout)?.[1];
    const lastBackup = lastSyncTimeStr != null ? new Date(lastSyncTimeStr) : undefined;
    const workspaceChoices = cloudListOutput.stdout
      .split('\n')
      .map(line => /^ {8}([a-zA-Z/0-9._-]+)(?: \(connected\))?/.exec(line)?.[1] as string)
      .filter(l => l != null);

    return {
      lastBackup,
      currentWorkspace,
      workspaceChoices,
    };
  }

  private commitCache = new LRU<string, CommitInfo>(100); // TODO: normal commits fetched from smartlog() aren't put in this cache---this is mostly for blame right now.
  public async lookupCommits(
    ctx: RepositoryContext,
    hashes: Array<string>,
  ): Promise<Map<string, CommitInfo>> {
    const hashesToFetch = hashes.filter(hash => this.commitCache.get(hash) == undefined);

    const commits =
      hashesToFetch.length === 0
        ? [] // don't bother running log
        : await this.runCommand(
            ['log', '--template', FETCH_TEMPLATE, '--rev', hashesToFetch.join('+')],
            'LookupCommitsCommand',
            ctx,
          ).then(output => {
            return parseCommitInfoOutput(ctx.logger, output.stdout.trim());
          });

    const result = new Map();
    for (const hash of hashes) {
      const found = this.commitCache.get(hash);
      if (found != undefined) {
        result.set(hash, found);
      }
    }

    for (const commit of commits) {
      if (commit) {
        this.commitCache.set(commit.hash, commit);
        result.set(commit.hash, commit);
      }
    }

    return result;
  }

  public async getAllChangedFiles(ctx: RepositoryContext, hash: Hash): Promise<Array<ChangedFile>> {
    const output = (
      await this.runCommand(
        ['log', '--template', CHANGED_FILES_TEMPLATE, '--rev', hash],
        'LookupAllCommitChangedFilesCommand',
        ctx,
      )
    ).stdout;

    const [chunk] = output.split(COMMIT_END_MARK, 1);

    const lines = chunk.trim().split('\n');
    if (lines.length < Object.keys(CHANGED_FILES_FIELDS).length) {
      return [];
    }

    const files: Array<ChangedFile> = [
      ...(JSON.parse(lines[CHANGED_FILES_INDEX.filesModified]) as Array<string>).map(path => ({
        path,
        status: 'M' as const,
      })),
      ...(JSON.parse(lines[CHANGED_FILES_INDEX.filesAdded]) as Array<string>).map(path => ({
        path,
        status: 'A' as const,
      })),
      ...(JSON.parse(lines[CHANGED_FILES_INDEX.filesRemoved]) as Array<string>).map(path => ({
        path,
        status: 'R' as const,
      })),
    ];

    return files;
  }

  public async getShelvedChanges(ctx: RepositoryContext): Promise<Array<ShelvedChange>> {
    const output = (
      await this.runCommand(
        ['log', '--rev', 'shelved()', '--template', SHELVE_FETCH_TEMPLATE],
        'GetShelvesCommand',
        ctx,
      )
    ).stdout;

    const shelves = parseShelvedCommitsOutput(ctx.logger, output.trim());
    // sort by date ascending
    shelves.sort((a, b) => b.date.getTime() - a.date.getTime());
    return shelves;
  }

  public getAllDiffIds(): Array<DiffId> {
    return (
      this.getSmartlogCommits()
        ?.commits.value?.map(commit => commit.diffId)
        .filter(notEmpty) ?? []
    );
  }

  public async getActiveAlerts(ctx: RepositoryContext): Promise<Array<Alert>> {
    const result = await this.runCommand(['config', '-Tjson', 'alerts'], 'GetAlertsCommand', ctx, {
      reject: false,
    });
    if (result.exitCode !== 0 || !result.stdout) {
      return [];
    }
    try {
      const configs = JSON.parse(result.stdout) as [{name: string; value: unknown}];
      const alerts = parseAlerts(configs);
      ctx.logger.info('Found active alerts:', alerts);
      return alerts;
    } catch (e) {
      return [];
    }
  }

  public async getRagePaste(ctx: RepositoryContext): Promise<string> {
    const output = await this.runCommand(['rage'], 'RageCommand', ctx, undefined, 90_000);
    const match = /P\d{9,}/.exec(output.stdout);
    if (match) {
      return match[0];
    }
    throw new Error('No paste found in rage output: ' + output.stdout);
  }

  public async runDiff(
    ctx: RepositoryContext,
    comparison: Comparison,
    contextLines = 4,
  ): Promise<string> {
    const output = await this.runCommand(
      [
        'diff',
        ...revsetArgsForComparison(comparison),
        // don't include a/ and b/ prefixes on files
        '--noprefix',
        '--no-binary',
        '--nodate',
        '--unified',
        String(contextLines),
      ],
      'DiffCommand',
      ctx,
    );
    return output.stdout;
  }

  public runCommand(
    args: Array<string>,
    /** Which event name to track for this command. If undefined, generic 'RunCommand' is used. */
    eventName: TrackEventName | undefined,
    ctx: RepositoryContext,
    options?: execa.Options,
    timeout?: number,
  ) {
    const id = randomId();
    return ctx.tracker.operation(
      eventName ?? 'RunCommand',
      'RunCommandError',
      {
        // if we don't specify a specific eventName, provide the command arguments in logging
        extras: eventName == null ? {args} : undefined,
        operationId: `isl:${id}`,
      },
      () =>
        runCommand(
          ctx,
          args,
          {
            ...options,
            env: {...options?.env, ...Internal.additionalEnvForCommand?.(id)} as NodeJS.ProcessEnv,
          },
          timeout ?? READ_COMMAND_TIMEOUT_MS,
        ),
    );
  }

  /** Read a config. The config name must be part of `allConfigNames`. */
  public async getConfig(
    ctx: RepositoryContext,
    configName: ConfigName,
  ): Promise<string | undefined> {
    return (await this.getKnownConfigs(ctx)).get(configName);
  }

  /**
   * Read a single config, forcing a new dedicated call to `sl config`.
   * Prefer `getConfig` to batch fetches when possible.
   */
  public async forceGetConfig(
    ctx: RepositoryContext,
    configName: string,
  ): Promise<string | undefined> {
    const result = (await runCommand(ctx, ['config', configName])).stdout;
    this.initialConnectionContext.logger.info(
      `loaded configs from ${ctx.cwd}: ${configName} => ${result}`,
    );
    return result;
  }

  /** Load all "known" configs. Cached on `this`. */
  public getKnownConfigs(
    ctx: RepositoryContext,
  ): Promise<ReadonlyMap<ConfigName, string | undefined>> {
    if (ctx.knownConfigs != null) {
      return Promise.resolve(ctx.knownConfigs);
    }
    return this.configRateLimiter.enqueueRun(async () => {
      if (ctx.knownConfigs == null) {
        // Fetch all configs using one command.
        const knownConfig = new Map<ConfigName, string>(
          await getConfigs<ConfigName>(ctx, allConfigNames),
        );
        ctx.knownConfigs = knownConfig;
      }
      return ctx.knownConfigs;
    });
  }

  public setConfig(
    ctx: RepositoryContext,
    level: ConfigLevel,
    configName: SettableConfigName,
    configValue: string,
  ): Promise<void> {
    if (!settableConfigNames.includes(configName)) {
      return Promise.reject(
        new Error(`config ${configName} not in allowlist for settable configs`),
      );
    }
    // Attempt to avoid racy config read/write.
    return this.configRateLimiter.enqueueRun(() => setConfig(ctx, level, configName, configValue));
  }

  /** Load and apply configs to `this` in background. */
  private applyConfigInBackground(ctx: RepositoryContext) {
    this.getConfig(ctx, 'isl.hold-off-refresh-ms').then(configValue => {
      if (configValue != null) {
        const numberValue = parseInt(configValue, 10);
        if (numberValue >= 0) {
          this.configHoldOffRefreshMs = numberValue;
        }
      }
    });
  }
}

export function repoRelativePathForAbsolutePath(
  absolutePath: AbsolutePath,
  repo: Repository,
  pathMod = path,
): RepoRelativePath {
  return pathMod.relative(repo.info.repoRoot, absolutePath);
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
