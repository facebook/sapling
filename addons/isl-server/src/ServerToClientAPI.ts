/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TypeaheadResult} from 'isl-components/Types';
import type {Serializable} from 'isl/src/serialize';
import type {
  ClientToServerMessage,
  CodeReviewProviderSpecificClientToServerMessages,
  Disposable,
  FetchedCommits,
  FetchedUncommittedChanges,
  FileABugProgress,
  LandInfo,
  MergeConflicts,
  PlatformSpecificClientToServerMessages,
  RepositoryError,
  Result,
  ServerToClientMessage,
  StableLocationData,
  SubmodulesByRoot,
} from 'isl/src/types';
import type {EjecaError, EjecaReturn} from 'shared/ejeca';
import type {ExportStack, ImportedStack} from 'shared/types/stack';
import type {ClientConnection} from '.';
import type {RepositoryReference} from './RepositoryCache';
import type {ServerSideTracker} from './analytics/serverSideTracker';
import type {Logger} from './logger';
import type {ServerPlatform} from './serverPlatform';
import type {RepositoryContext} from './serverTypes';

import type {InternalTypes} from 'isl/src/InternalTypes';
import {deserializeFromString, serializeToString} from 'isl/src/serialize';
import type {PartiallySelectedDiffCommit} from 'isl/src/stackEdit/diffSplitTypes';
import {Readable} from 'node:stream';
import path from 'path';
import {beforeRevsetForComparison} from 'shared/Comparison';
import {base64Decode, notEmpty, randomId} from 'shared/utils';
import {generatedFilesDetector} from './GeneratedFiles';
import {Internal} from './Internal';
import {Repository, absolutePathForFileInRepo} from './Repository';
import {repositoryCache} from './RepositoryCache';
import {firstOfIterable, parseExecJson} from './utils';

export type IncomingMessage = ClientToServerMessage;
export type OutgoingMessage = ServerToClientMessage;

type GeneralMessage = IncomingMessage &
  (
    | {type: 'heartbeat'}
    | {type: 'stress'}
    | {type: 'changeCwd'}
    | {type: 'requestRepoInfo'}
    | {type: 'requestApplicationInfo'}
    | {type: 'fileBugReport'}
    | {type: 'track'}
    | {type: 'clientReady'}
  );
type WithRepoMessage = Exclude<IncomingMessage, GeneralMessage>;

/**
 * Message passing channel built on top of ClientConnection.
 * Use to send and listen for well-typed events with the client
 *
 * Note: you must set the current repository to start sending data back to the client.
 */
export default class ServerToClientAPI {
  private listenersByType = new Map<
    string,
    Set<(message: IncomingMessage) => void | Promise<void>>
  >();
  private incomingListener: Disposable;

  /** Disposables that must be disposed whenever the current repo is changed */
  private repoDisposables: Array<Disposable> = [];
  private subscriptions = new Map<string, Disposable>();
  private activeRepoRef: RepositoryReference | undefined;

  private queuedMessages: Array<IncomingMessage> = [];
  private currentState:
    | {type: 'loading'}
    | {type: 'repo'; repo: Repository; ctx: RepositoryContext}
    | {type: 'error'; error: RepositoryError} = {type: 'loading'};

  private pageId = randomId();

  constructor(
    private platform: ServerPlatform,
    private connection: ClientConnection,
    private tracker: ServerSideTracker,
    private logger: Logger,
  ) {
    this.incomingListener = this.connection.onDidReceiveMessage(buf => {
      const message = buf.toString('utf-8');
      const data = deserializeFromString(message) as IncomingMessage;

      // When the client is connected, we want to immediately start listening to messages.
      // However, we can't properly respond to these messages until we have a repository set up.
      // Queue up messages until a repository is set.
      if (this.currentState.type === 'loading') {
        this.queuedMessages.push(data);
      } else {
        try {
          this.handleIncomingMessage(data);
        } catch (err) {
          connection.logger?.error('error handling incoming message: ', data, err);
        }
      }
    });
  }

  private setRepoError(error: RepositoryError) {
    this.disposeRepoDisposables();

    this.currentState = {type: 'error', error};

    this.tracker.context.setRepo(undefined);

    this.processQueuedMessages();
  }

  private setCurrentRepo(repo: Repository, ctx: RepositoryContext) {
    this.disposeRepoDisposables();

    this.currentState = {type: 'repo', repo, ctx};

    this.tracker.context.setRepo(repo);

    if (repo.codeReviewProvider != null) {
      this.repoDisposables.push(
        repo.codeReviewProvider.onChangeDiffSummaries(value => {
          this.postMessage({type: 'fetchedDiffSummaries', summaries: value});
        }),
      );
    }

    repo.ref();
    this.repoDisposables.push({dispose: () => repo.unref()});

    this.processQueuedMessages();
  }

  postMessage(message: OutgoingMessage) {
    this.connection.postMessage(serializeToString(message));
  }

  /** Get a repository reference for a given cwd, and set that as the active repo. */
  setActiveRepoForCwd(newCwd: string) {
    if (this.activeRepoRef !== undefined) {
      this.activeRepoRef.unref();
    }
    this.logger.info(`Setting active repo cwd to ${newCwd}`);
    // Set as loading right away while we determine the new cwd's repo
    // This ensures new messages coming in will be queued and handled only with the new repository
    this.currentState = {type: 'loading'};
    const command = this.connection.command ?? 'sl';
    const ctx: RepositoryContext = {
      cwd: newCwd,
      cmd: command,
      logger: this.logger,
      tracker: this.tracker,
    };
    this.activeRepoRef = repositoryCache.getOrCreate(ctx);
    this.activeRepoRef.promise.then(repoOrError => {
      if (repoOrError instanceof Repository) {
        this.setCurrentRepo(repoOrError, ctx);
      } else {
        this.setRepoError(repoOrError);
      }
    });
  }

  dispose() {
    this.incomingListener.dispose();
    this.disposeRepoDisposables();

    if (this.activeRepoRef !== undefined) {
      this.activeRepoRef.unref();
    }
  }

  private disposeRepoDisposables() {
    this.repoDisposables.forEach(disposable => disposable.dispose());
    this.repoDisposables = [];

    this.subscriptions.forEach(sub => sub.dispose());
    this.subscriptions.clear();
  }

  private processQueuedMessages() {
    for (const message of this.queuedMessages) {
      try {
        this.handleIncomingMessage(message);
      } catch (err) {
        this.connection.logger?.error('error handling queued message: ', message, err);
      }
    }
    this.queuedMessages = [];
  }

  private handleIncomingMessage(data: IncomingMessage) {
    this.handleIncomingGeneralMessage(data as GeneralMessage);
    const {currentState} = this;
    switch (currentState.type) {
      case 'repo': {
        const {repo, ctx} = currentState;
        this.handleIncomingMessageWithRepo(data as WithRepoMessage, repo, ctx);
        break;
      }

      // If the repo is in the loading or error state, the client may still send
      // platform messages such as `platform/openExternal` that should be processed.
      case 'loading':
      case 'error':
        if (data.type.startsWith('platform/')) {
          this.platform.handleMessageFromClient(
            /*repo=*/ undefined,
            // even if we don't have a repo, we can still make a RepositoryContext to execute commands
            {
              cwd: this.connection.cwd,
              cmd: this.connection.command ?? 'sl',
              logger: this.logger,
              tracker: this.tracker,
            },
            data as PlatformSpecificClientToServerMessages,
            message => this.postMessage(message),
            (dispose: () => unknown) => {
              this.repoDisposables.push({dispose});
            },
          );
          this.notifyListeners(data);
        }
        break;
    }
  }

  /**
   * Handle messages which can be handled regardless of if a repo was successfully created or not
   */
  private handleIncomingGeneralMessage(data: GeneralMessage) {
    switch (data.type) {
      case 'heartbeat': {
        this.postMessage({type: 'heartbeat', id: data.id});
        break;
      }
      case 'stress': {
        this.postMessage(data);
        break;
      }
      case 'track': {
        this.tracker.trackData(data.data);
        break;
      }
      case 'clientReady': {
        this.connection.readySignal?.resolve();
        break;
      }
      case 'changeCwd': {
        this.setActiveRepoForCwd(data.cwd);
        break;
      }
      case 'requestRepoInfo': {
        switch (this.currentState.type) {
          case 'repo':
            this.postMessage({
              type: 'repoInfo',
              info: this.currentState.repo.info,
              cwd: this.currentState.ctx.cwd,
            });
            break;
          case 'error':
            this.postMessage({type: 'repoInfo', info: this.currentState.error});
            break;
        }
        break;
      }
      case 'requestApplicationInfo': {
        this.postMessage({
          type: 'applicationInfo',
          info: {
            platformName: this.platform.platformName,
            version: this.connection.version,
            logFilePath: this.connection.logFileLocation ?? '(no log file, logging to stdout)',
          },
        });
        break;
      }
      case 'fileBugReport': {
        const maybeRepo = this.currentState.type === 'repo' ? this.currentState.repo : undefined;
        const ctx: RepositoryContext =
          this.currentState.type === 'repo'
            ? this.currentState.ctx
            : {
                // cwd is only needed to run graphql query, here it's just best-effort
                cwd: maybeRepo?.initialConnectionContext.cwd ?? process.cwd(),
                cmd: this.connection.command ?? 'sl',
                logger: this.logger,
                tracker: this.tracker,
              };
        Internal.fileABug?.(
          ctx,
          this.platform.platformName,
          data.data,
          data.uiState,
          // Use repo for rage, if available.
          maybeRepo,
          data.collectRage,
          (progress: FileABugProgress) => {
            this.connection.logger?.info('file a bug progress: ', JSON.stringify(progress));
            this.postMessage({type: 'fileBugReportProgress', ...progress});
          },
        );
        break;
      }
    }
  }

  private handleMaybeForgotOperation(operationId: string, repo: Repository) {
    if (repo.getRunningOperation()?.id !== operationId) {
      this.postMessage({type: 'operationProgress', id: operationId, kind: 'forgot'});
    }
  }

  /**
   * Handle messages which require a repository to have been successfully set up to run
   */
  private handleIncomingMessageWithRepo(
    data: WithRepoMessage,
    repo: Repository,
    ctx: RepositoryContext,
  ) {
    const {cwd, logger} = ctx;
    switch (data.type) {
      case 'subscribe': {
        const {subscriptionID, kind} = data;
        switch (kind) {
          case 'uncommittedChanges': {
            const postUncommittedChanges = (result: FetchedUncommittedChanges) => {
              this.postMessage({
                type: 'subscriptionResult',
                kind: 'uncommittedChanges',
                subscriptionID,
                data: result,
              });
            };

            const uncommittedChanges = repo.getUncommittedChanges();
            if (uncommittedChanges != null) {
              postUncommittedChanges(uncommittedChanges);
            }
            const disposables: Array<Disposable> = [];

            // send changes as they come in from watchman
            disposables.push(repo.subscribeToUncommittedChanges(postUncommittedChanges));
            // trigger a fetch on startup
            repo.fetchUncommittedChanges();

            disposables.push(
              repo.subscribeToUncommittedChangesBeginFetching(() =>
                this.postMessage({type: 'beganFetchingUncommittedChangesEvent'}),
              ),
            );
            this.subscriptions.set(subscriptionID, {
              dispose: () => {
                disposables.forEach(d => d.dispose());
              },
            });
            break;
          }
          case 'smartlogCommits': {
            const postSmartlogCommits = (result: FetchedCommits) => {
              this.postMessage({
                type: 'subscriptionResult',
                kind: 'smartlogCommits',
                subscriptionID,
                data: result,
              });
            };

            const smartlogCommits = repo.getSmartlogCommits();
            if (smartlogCommits != null) {
              postSmartlogCommits(smartlogCommits);
            }
            const disposables: Array<Disposable> = [];
            // send changes as they come from file watcher
            disposables.push(repo.subscribeToSmartlogCommitsChanges(postSmartlogCommits));
            // trigger a fetch on startup
            repo.fetchSmartlogCommits();

            disposables.push(
              repo.subscribeToSmartlogCommitsBeginFetching(() =>
                this.postMessage({type: 'beganFetchingSmartlogCommitsEvent'}),
              ),
            );

            this.subscriptions.set(subscriptionID, {
              dispose: () => {
                disposables.forEach(d => d.dispose());
              },
            });
            break;
          }
          case 'mergeConflicts': {
            const postMergeConflicts = (conflicts: MergeConflicts | undefined) => {
              this.postMessage({
                type: 'subscriptionResult',
                kind: 'mergeConflicts',
                subscriptionID,
                data: conflicts,
              });
            };

            const mergeConflicts = repo.getMergeConflicts();
            if (mergeConflicts != null) {
              postMergeConflicts(mergeConflicts);
            }

            this.subscriptions.set(subscriptionID, repo.onChangeConflictState(postMergeConflicts));
            break;
          }
          case 'submodules': {
            const postSubmodules = (submodulesByRoot: SubmodulesByRoot) => {
              this.postMessage({
                type: 'subscriptionResult',
                kind: 'submodules',
                subscriptionID,
                data: submodulesByRoot,
              });
            };
            const submoduleMap = repo.getSubmoduleMap();
            if (submoduleMap !== undefined) {
              postSubmodules(submoduleMap);
            }
            repo.fetchSubmoduleMap();

            const disposable = repo.subscribeToSubmodulesChanges(postSubmodules);
            this.subscriptions.set(subscriptionID, {
              dispose: () => {
                disposable.dispose();
              },
            });
            break;
          }
        }
        break;
      }
      case 'unsubscribe': {
        const subscription = this.subscriptions.get(data.subscriptionID);
        subscription?.dispose();
        this.subscriptions.delete(data.subscriptionID);
        break;
      }
      case 'runOperation': {
        const {operation} = data;
        repo.runOrQueueOperation(ctx, operation, progress => {
          this.postMessage({type: 'operationProgress', ...progress});
          if (progress.kind === 'queue') {
            this.tracker.track('QueueOperation', {extras: {operation: operation.trackEventName}});
          }
        });
        break;
      }
      case 'abortRunningOperation': {
        const {operationId} = data;
        repo.abortRunningOpeation(operationId);
        this.handleMaybeForgotOperation(operationId, repo);
        break;
      }
      case 'getConfig': {
        repo
          .getConfig(ctx, data.name)
          .catch(() => undefined)
          .then(value => {
            logger.info('got config', data.name, value);
            this.postMessage({type: 'gotConfig', name: data.name, value});
          });
        break;
      }
      case 'setConfig': {
        logger.info('set config', data.name, data.value);
        repo.setConfig(ctx, 'user', data.name, data.value).catch(err => {
          logger.error('error setting config', data.name, data.value, err);
        });
        break;
      }
      case 'setDebugLogging': {
        logger.info('set debug', data.name, data.enabled);
        if (data.name === 'debug' || data.name === 'verbose') {
          ctx[data.name] = !!data.enabled;
        }
        break;
      }
      case 'requestComparison': {
        const {comparison} = data;
        const diff: Promise<Result<string>> = repo
          .runDiff(ctx, comparison)
          .then(value => ({value}))
          .catch(error => {
            logger?.error('error running diff', error.toString());
            return {error};
          });
        diff.then(data =>
          this.postMessage({
            type: 'comparison',
            comparison,
            data: {diff: data},
          }),
        );
        break;
      }
      case 'requestComparisonContextLines': {
        const {
          id: {path: relativePath, comparison},
          // This is the line number in the "before" side of the comparison
          start,
          // This is the number of context lines to fetch
          numLines,
        } = data;

        const absolutePath = path.join(repo.info.repoRoot, relativePath);

        // TODO: For context lines, before/after sides of the comparison
        // are identical... except for line numbers.
        // Typical comparisons with '.' would be much faster (nearly instant)
        // by reading from the filesystem rather than using cat,
        // we just need the caller to ask with "after" line numbers instead of "before".
        // Note: we would still need to fall back to cat for comparisons that do not involve
        // the working copy.
        const cat: Promise<string> = repo.cat(
          ctx,
          absolutePath,
          beforeRevsetForComparison(comparison),
        );

        cat
          .then(content =>
            this.postMessage({
              type: 'comparisonContextLines',
              lines: {value: content.split('\n').slice(start - 1, start - 1 + numLines)},
              path: relativePath,
            }),
          )
          .catch((error: Error) =>
            this.postMessage({
              type: 'comparisonContextLines',
              lines: {error},
              path: relativePath,
            }),
          );
        break;
      }
      case 'requestMissedOperationProgress': {
        const {operationId} = data;
        this.handleMaybeForgotOperation(operationId, repo);
        break;
      }
      case 'refresh': {
        logger?.log('refresh requested');
        repo.fetchSmartlogCommits();
        repo.fetchUncommittedChanges();
        repo.checkForMergeConflicts();
        repo.codeReviewProvider?.triggerDiffSummariesFetch(repo.getAllDiffIds());
        generatedFilesDetector.clear(); // allow generated files to be rechecked
        break;
      }
      case 'pageVisibility': {
        repo.setPageFocus(this.pageId, data.state);
        break;
      }
      case 'uploadFile': {
        const {id, filename, b64Content} = data;
        const payload = base64Decode(b64Content);
        const uploadFile = Internal.uploadFile;
        if (uploadFile == null) {
          return;
        }
        this.tracker
          .operation('UploadImage', 'UploadImageError', {}, () =>
            uploadFile(this.logger, {filename, data: payload}),
          )
          .then((result: string) => {
            this.logger.info('sucessfully uploaded file', filename, result);
            this.postMessage({type: 'uploadFileResult', id, result: {value: result}});
          })
          .catch((error: Error) => {
            this.logger.info('error uploading file', filename, error);
            this.postMessage({type: 'uploadFileResult', id, result: {error}});
          });
        break;
      }
      case 'fetchCommitMessageTemplate': {
        this.handleFetchCommitMessageTemplate(repo, ctx);
        break;
      }
      case 'fetchShelvedChanges': {
        repo
          .getShelvedChanges(ctx)
          .then(shelvedChanges => {
            this.postMessage({
              type: 'fetchedShelvedChanges',
              shelvedChanges: {value: shelvedChanges},
            });
          })
          .catch(err => {
            logger?.error('Could not fetch shelved changes', err);
            this.postMessage({type: 'fetchedShelvedChanges', shelvedChanges: {error: err}});
          });
        break;
      }
      case 'fetchLatestCommit': {
        repo
          .lookupCommits(ctx, [data.revset])
          .then(commits => {
            const commit = firstOfIterable(commits.values());
            if (commit == null) {
              throw new Error(`No commit found for revset ${data.revset}`);
            }
            this.postMessage({
              type: 'fetchedLatestCommit',
              revset: data.revset,
              info: {value: commit},
            });
          })
          .catch(err => {
            this.postMessage({
              type: 'fetchedLatestCommit',
              revset: data.revset,
              info: {error: err as Error},
            });
          });
        break;
      }
      case 'fetchPendingSignificantLinesOfCode':
        {
          repo
            .fetchPendingSignificantLinesOfCode(ctx, data.hash, data.includedFiles)
            .then(value => {
              this.postMessage({
                type: 'fetchedPendingSignificantLinesOfCode',
                requestId: data.requestId,
                hash: data.hash,
                result: {value: value ?? 0},
              });
            })
            .catch(err => {
              this.postMessage({
                type: 'fetchedPendingSignificantLinesOfCode',
                hash: data.hash,
                requestId: data.requestId,
                result: {error: err as Error},
              });
            });
        }
        break;
      case 'fetchSignificantLinesOfCode':
        {
          repo
            .fetchSignificantLinesOfCode(ctx, data.hash, data.excludedFiles)
            .then(value => {
              this.postMessage({
                type: 'fetchedSignificantLinesOfCode',
                hash: data.hash,
                result: {value: value ?? 0},
              });
            })
            .catch(err => {
              this.postMessage({
                type: 'fetchedSignificantLinesOfCode',
                hash: data.hash,
                result: {error: err as Error},
              });
            });
        }
        break;
      case 'fetchPendingAmendSignificantLinesOfCode':
        {
          repo
            .fetchPendingAmendSignificantLinesOfCode(ctx, data.hash, data.includedFiles)
            .then(value => {
              this.postMessage({
                type: 'fetchedPendingAmendSignificantLinesOfCode',
                requestId: data.requestId,
                hash: data.hash,
                result: {value: value ?? 0},
              });
            })
            .catch(err => {
              this.postMessage({
                type: 'fetchedPendingAmendSignificantLinesOfCode',
                hash: data.hash,
                requestId: data.requestId,
                result: {error: err as Error},
              });
            });
        }
        break;
      case 'fetchCommitChangedFiles': {
        repo
          .getAllChangedFiles(ctx, data.hash)
          .then(files => {
            this.postMessage({
              type: 'fetchedCommitChangedFiles',
              hash: data.hash,
              result: {
                value: {
                  filesSample: data.limit != null ? files.slice(0, data.limit) : files,
                  totalFileCount: files.length,
                },
              },
            });
          })
          .catch(err => {
            this.postMessage({
              type: 'fetchedCommitChangedFiles',
              hash: data.hash,
              result: {error: err as Error},
            });
          });
        break;
      }
      case 'fetchCommitCloudState': {
        repo.getCommitCloudState(ctx).then(state => {
          this.postMessage({
            type: 'fetchedCommitCloudState',
            state: {value: state},
          });
        });
        break;
      }
      case 'fetchGeneratedStatuses': {
        generatedFilesDetector
          .queryFilesGenerated(repo, ctx, repo.info.repoRoot, data.paths)
          .then(results => {
            this.postMessage({type: 'fetchedGeneratedStatuses', results});
          });
        break;
      }
      case 'typeahead': {
        // Current repo's code review provider should be able to handle all
        // TypeaheadKinds for the fields in its defined schema.
        repo.codeReviewProvider?.typeahead?.(data.kind, data.query, cwd)?.then(result =>
          this.postMessage({
            type: 'typeaheadResult',
            id: data.id,
            result,
          }),
        );
        break;
      }
      case 'fetchDiffSummaries': {
        repo.codeReviewProvider?.triggerDiffSummariesFetch(data.diffIds ?? repo.getAllDiffIds());
        break;
      }
      case 'fetchLandInfo': {
        repo.codeReviewProvider
          ?.fetchLandInfo?.(data.topOfStack)
          ?.then((landInfo: LandInfo) => {
            this.postMessage({
              type: 'fetchedLandInfo',
              topOfStack: data.topOfStack,
              landInfo: {value: landInfo},
            });
          })
          .catch(err => {
            this.postMessage({
              type: 'fetchedLandInfo',
              topOfStack: data.topOfStack,
              landInfo: {error: err as Error},
            });
          });

        break;
      }
      case 'confirmLand': {
        if (data.landConfirmationInfo == null) {
          break;
        }
        repo.codeReviewProvider
          ?.confirmLand?.(data.landConfirmationInfo)
          ?.then((result: Result<undefined>) => {
            this.postMessage({
              type: 'confirmedLand',
              result,
            });
          });
        break;
      }
      case 'fetchAvatars': {
        repo.codeReviewProvider?.fetchAvatars?.(data.authors)?.then(avatars => {
          this.postMessage({
            type: 'fetchedAvatars',
            avatars,
            authors: data.authors,
          });
        });
        break;
      }
      case 'fetchDiffComments': {
        repo.codeReviewProvider
          ?.fetchComments?.(data.diffId)
          ?.then(comments => {
            this.postMessage({
              type: 'fetchedDiffComments',
              diffId: data.diffId,
              comments: {value: comments},
            });
          })
          .catch(error => {
            this.postMessage({
              type: 'fetchedDiffComments',
              diffId: data.diffId,
              comments: {error},
            });
          });
        break;
      }
      case 'renderMarkup': {
        repo.codeReviewProvider
          ?.renderMarkup?.(data.markup)
          ?.then(html => {
            this.postMessage({
              type: 'renderedMarkup',
              id: data.id,
              html,
            });
          })
          ?.catch(err => {
            this.logger.error('Error rendering markup:', err);
          });
        break;
      }
      case 'getSuggestedReviewers': {
        repo.codeReviewProvider?.getSuggestedReviewers?.(data.context).then(reviewers => {
          this.postMessage({
            type: 'gotSuggestedReviewers',
            reviewers,
            key: data.key,
          });
        });
        break;
      }
      case 'updateRemoteDiffMessage': {
        repo.codeReviewProvider
          ?.updateDiffMessage?.(data.diffId, data.title, data.description)
          ?.catch(err => err)
          ?.then((error: string | undefined) => {
            if (error != null) {
              this.logger.error('Error updating remote diff message:', error);
            }
            this.postMessage({type: 'updatedRemoteDiffMessage', diffId: data.diffId, error});
          });
        break;
      }
      case 'loadMoreCommits': {
        const rangeInDays = repo.nextVisibleCommitRangeInDays();
        this.postMessage({type: 'commitsShownRange', rangeInDays});
        this.postMessage({type: 'beganLoadingMoreCommits'});
        repo.fetchSmartlogCommits();
        this.tracker.track('LoadMoreCommits', {extras: {daysToFetch: rangeInDays ?? 'Infinity'}});
        return;
      }
      case 'exportStack': {
        const {revs, assumeTracked} = data;
        const assumeTrackedArgs = (assumeTracked ?? []).map(path => `--assume-tracked=${path}`);
        const exec = repo.runCommand(
          ['debugexportstack', '-r', revs, ...assumeTrackedArgs],
          'ExportStackCommand',
          ctx,
          undefined,
          /* don't timeout */ 0,
        );
        const reply = (stack?: ExportStack, error?: string) => {
          this.postMessage({
            type: 'exportedStack',
            assumeTracked: assumeTracked ?? [],
            revs,
            stack: stack ?? [],
            error,
          });
        };
        parseExecJson(exec, reply);
        break;
      }
      case 'importStack': {
        const stdinStream = Readable.from(JSON.stringify(data.stack));
        const exec = repo.runCommand(
          ['debugimportstack'],
          'ImportStackCommand',
          ctx,
          {stdin: stdinStream},
          /* don't timeout */ 0,
        );
        const reply = (imported?: ImportedStack, error?: string) => {
          this.postMessage({type: 'importedStack', imported: imported ?? [], error});
        };
        parseExecJson(exec, reply);
        break;
      }
      case 'fetchQeFlag': {
        Internal.fetchQeFlag?.(repo.initialConnectionContext, data.name).then((passes: boolean) => {
          this.logger.info(`qe flag ${data.name} ${passes ? 'PASSES' : 'FAILS'}`);
          this.postMessage({type: 'fetchedQeFlag', name: data.name, passes});
        });
        break;
      }
      case 'fetchFeatureFlag': {
        Internal.fetchFeatureFlag?.(repo.initialConnectionContext, data.name).then(
          (passes: boolean) => {
            this.logger.info(`feature flag ${data.name} ${passes ? 'PASSES' : 'FAILS'}`);
            this.postMessage({type: 'fetchedFeatureFlag', name: data.name, passes});
          },
        );
        break;
      }
      case 'bulkFetchFeatureFlags': {
        Internal.bulkFetchFeatureFlags?.(repo.initialConnectionContext, data.names).then(
          (result: Record<string, boolean>) => {
            this.logger.info(`feature flags ${JSON.stringify(result, null, 2)}`);
            this.postMessage({type: 'bulkFetchedFeatureFlags', id: data.id, result});
          },
        );
        break;
      }
      case 'fetchInternalUserInfo': {
        Internal.fetchUserInfo?.(repo.initialConnectionContext).then((info: Serializable) => {
          this.logger.info('user info:', info);
          this.postMessage({type: 'fetchedInternalUserInfo', info});
        });
        break;
      }
      case 'fetchAndSetStables': {
        Internal.fetchStableLocations?.(ctx, data.additionalStables).then(
          (stables: StableLocationData | undefined) => {
            this.logger.info('fetched stable locations', stables);
            if (stables == null) {
              return;
            }
            this.postMessage({type: 'fetchedStables', stables});
            repo.stableLocations = [
              ...stables.stables,
              ...stables.special,
              ...Object.values(stables.manual),
            ]
              .map(stable => stable?.value)
              .filter(notEmpty);
            repo.fetchSmartlogCommits();
          },
        );
        break;
      }
      case 'fetchStableLocationAutocompleteOptions': {
        Internal.fetchStableLocationAutocompleteOptions?.(ctx).then(
          (result: Result<Array<TypeaheadResult>>) => {
            this.postMessage({type: 'fetchedStableLocationAutocompleteOptions', result});
          },
        );
        break;
      }
      case 'fetchDevEnvType': {
        if (Internal.getDevEnvType == null) {
          break;
        }

        Internal.getDevEnvType()
          .catch((error: Error) => {
            this.logger.error('Error getting dev env type:', error);
            return 'error';
          })
          .then((result: string) => {
            this.postMessage({
              type: 'fetchedDevEnvType',
              envType: result,
              id: data.id,
            });
          });
        break;
      }
      case 'generateSuggestionWithAI': {
        if (Internal.generateSuggestionWithAI == null) {
          break;
        }
        repo.runDiff(ctx, data.comparison, /* context lines */ 4).then(diff => {
          Internal.generateSuggestionWithAI?.(repo.initialConnectionContext, {
            context: diff,
            fieldName: data.fieldName,
            latestFields: data.latestFields,
            suggestionId: data.suggestionId,
          })
            .catch((error: Error) => ({error}))
            .then((result: Result<string>) => {
              this.postMessage({
                type: 'generatedSuggestionWithAI',
                message: result,
                id: data.id,
              });
            });
        });
        break;
      }
      case 'splitCommitWithAI': {
        Internal.splitCommitWithAI?.(ctx, data.diffCommit, data.args).then(
          (result: Result<ReadonlyArray<PartiallySelectedDiffCommit>>) => {
            this.postMessage({
              type: 'splitCommitWithAI',
              id: data.id,
              result,
            });
          },
        );
        break;
      }
      case 'fetchActiveAlerts': {
        repo
          .getActiveAlerts(ctx)
          .then(alerts => {
            if (alerts.length === 0) {
              return;
            }
            this.postMessage({
              type: 'fetchedActiveAlerts',
              alerts,
            });
          })
          .catch(err => {
            this.logger.error('Failed to fetch active alerts:', err);
          });
        break;
      }
      case 'gotUiState': {
        break;
      }
      case 'getConfiguredMergeTool': {
        repo.getMergeTool(ctx).then((tool: string | null) => {
          this.postMessage({
            type: 'gotConfiguredMergeTool',
            tool: tool ?? undefined,
          });
        });
        break;
      }
      case 'fetchGkDetails': {
        Internal.fetchGkDetails?.(ctx, data.name)
          .then((gk: InternalTypes['InternalGatekeeper']) => {
            this.postMessage({type: 'fetchedGkDetails', id: data.id, result: {value: gk}});
          })
          .catch((err: unknown) => {
            logger?.error('Could not fetch GK details', err);
            this.postMessage({
              type: 'fetchedGkDetails',
              id: data.id,
              result: {error: err as Error},
            });
          });
        break;
      }
      case 'fetchJkDetails': {
        Internal.fetchJustKnobsByNames?.(ctx, data.names)
          .then((jk: InternalTypes['InternalJustknob']) => {
            this.postMessage({type: 'fetchedJkDetails', id: data.id, result: {value: jk}});
          })
          .catch((err: unknown) => {
            logger?.error('Could not fetch JK details', err);
            this.postMessage({
              type: 'fetchedJkDetails',
              id: data.id,
              result: {error: err as Error},
            });
          });
        break;
      }
      case 'fetchKnobsetDetails': {
        Internal.fetchKnobset?.(ctx, data.configPath)
          .then((knobset: InternalTypes['InternalKnobset']) => {
            this.postMessage({
              type: 'fetchedKnobsetDetails',
              id: data.id,
              result: {value: knobset},
            });
          })
          .catch((err: unknown) => {
            logger?.error('Could not fetch knobset details', err);
            this.postMessage({
              type: 'fetchedKnobsetDetails',
              id: data.id,
              result: {error: err as Error},
            });
          });
        break;
      }
      case 'fetchQeDetails': {
        Internal.fetchQeMetadata?.(ctx, data.name)
          .then((qe: InternalTypes['InternalQuickExperiment']) => {
            this.postMessage({
              type: 'fetchedQeDetails',
              id: data.id,
              result: {value: qe},
            });
          })
          .catch((err: unknown) => {
            logger?.error('Could not fetch QE details', err);
            this.postMessage({
              type: 'fetchedQeDetails',
              id: data.id,
              result: {error: err as Error},
            });
          });
        break;
      }
      case 'fetchABPropDetails': {
        Internal.fetchABPropMetadata?.(ctx, data.name)
          .then((abprop: InternalTypes['InternalMetaConfig']) => {
            this.postMessage({
              type: 'fetchedABPropDetails',
              id: data.id,
              result: {value: abprop},
            });
          })
          .catch((err: unknown) => {
            logger?.error('Could not fetch ABProp details', err);
            this.postMessage({
              type: 'fetchedABPropDetails',
              id: data.id,
              result: {error: err as Error},
            });
          });
        break;
      }
      case 'getRepoUrlAtHash': {
        const args = ['url', '--rev', data.revset];
        // validate that the path is a valid file in repo
        if (data.path != null && absolutePathForFileInRepo(data.path, repo) != null) {
          args.push(`path:${data.path}`);
        }
        repo
          .runCommand(args, 'RepoUrlCommand', ctx)
          .then(result => {
            this.postMessage({
              type: 'gotRepoUrlAtHash',
              url: {value: result.stdout},
            });
          })
          .catch((err: EjecaError) => {
            this.logger.error('Failed to get repo url at hash:', err);
            this.postMessage({
              type: 'gotRepoUrlAtHash',
              url: {error: err},
            });
          });
        break;
      }
      case 'fetchTaskDetails': {
        Internal.getTask?.(ctx, data.taskNumber).then(
          (task: InternalTypes['InternalTaskDetails']) => {
            this.postMessage({type: 'fetchedTaskDetails', id: data.id, result: {value: task}});
          },
        );
        break;
      }
      case 'runDevmateCommand': {
        Internal.runDevmateCommand?.(data.args, data.cwd)
          .then((result: EjecaReturn) => {
            this.postMessage({
              type: 'devmateCommandResult',
              result: {type: 'value', stdout: result.stdout, requestId: data.requestId},
            });
          })
          .catch((error: EjecaError) => {
            this.postMessage({
              type: 'devmateCommandResult',
              result: {type: 'error', stderr: error.stderr, requestId: data.requestId},
            });
          });
        break;
      }
      default: {
        if (
          repo.codeReviewProvider?.handleClientToServerMessage?.(data, message =>
            this.postMessage(message),
          ) === true
        ) {
          break;
        }
        this.platform.handleMessageFromClient(
          repo,
          ctx,
          data as Exclude<typeof data, CodeReviewProviderSpecificClientToServerMessages>,
          message => this.postMessage(message),
          (dispose: () => unknown) => {
            this.repoDisposables.push({dispose});
          },
        );
        break;
      }
    }

    this.notifyListeners(data);
  }

  private notifyListeners(data: IncomingMessage): void {
    const listeners = this.listenersByType.get(data.type);
    if (listeners) {
      listeners.forEach(handle => handle(data));
    }
  }

  private async handleFetchCommitMessageTemplate(repo: Repository, ctx: RepositoryContext) {
    const {logger} = ctx;
    try {
      const [result, customTemplate] = await Promise.all([
        repo.runCommand(['debugcommitmessage', 'isl'], 'FetchCommitTemplateCommand', ctx),
        Internal.getCustomDefaultCommitTemplate?.(repo.initialConnectionContext),
      ]);

      let template = result.stdout
        .replace(repo.IGNORE_COMMIT_MESSAGE_LINES_REGEX, '')
        .replace(/^<Replace this line with a title. Use 1 line only, 67 chars or less>/, '');

      if (customTemplate && customTemplate?.trim() !== '') {
        template = customTemplate as string;

        this.tracker.track('UseCustomCommitMessageTemplate');
      }

      this.postMessage({
        type: 'fetchedCommitMessageTemplate',
        template,
      });
    } catch (err) {
      logger?.error('Could not fetch commit message template', err);
    }
  }
}
