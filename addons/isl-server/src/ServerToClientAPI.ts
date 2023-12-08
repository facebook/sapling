/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ClientConnection} from '.';
import type {RepositoryReference} from './RepositoryCache';
import type {ServerSideTracker} from './analytics/serverSideTracker';
import type {Logger} from './logger';
import type {ServerPlatform} from './serverPlatform';
import type {Serializable} from 'isl/src/serialize';
import type {
  ServerToClientMessage,
  ClientToServerMessage,
  Disposable,
  Result,
  MergeConflicts,
  RepositoryError,
  PlatformSpecificClientToServerMessages,
  FileABugProgress,
  ClientToServerMessageWithPayload,
  FetchedCommits,
  FetchedUncommittedChanges,
  LandInfo,
  CodeReviewProviderSpecificClientToServerMessages,
} from 'isl/src/types';
import type {ExportStack, ImportedStack} from 'shared/types/stack';

import {generatedFilesDetector} from './GeneratedFiles';
import {Internal} from './Internal';
import {Repository} from './Repository';
import {repositoryCache} from './RepositoryCache';
import {findPublicAncestor, parseExecJson} from './utils';
import {serializeToString, deserializeFromString} from 'isl/src/serialize';
import {revsetForComparison} from 'shared/Comparison';
import {randomId, unwrap} from 'shared/utils';
import {Readable} from 'stream';

export type IncomingMessage = ClientToServerMessage;
type IncomingMessageWithPayload = ClientToServerMessageWithPayload;
export type OutgoingMessage = ServerToClientMessage;

type GeneralMessage = IncomingMessage &
  (
    | {type: 'heartbeat'}
    | {type: 'changeCwd'}
    | {type: 'requestRepoInfo'}
    | {type: 'requestApplicationInfo'}
    | {type: 'fileBugReport'}
    | {type: 'track'}
  );
type WithRepoMessage = Exclude<IncomingMessage, GeneralMessage>;

/**
 * Return true if a ClientToServerMessage is a ClientToServerMessageWithPayload
 */
function expectsBinaryPayload(message: unknown): message is ClientToServerMessageWithPayload {
  return (
    message != null &&
    typeof message === 'object' &&
    (message as ClientToServerMessageWithPayload).hasBinaryPayload === true
  );
}

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
    | {type: 'repo'; repo: Repository; cwd: string}
    | {type: 'error'; error: RepositoryError} = {type: 'loading'};

  private pageId = randomId();

  constructor(
    private platform: ServerPlatform,
    private connection: ClientConnection,
    private tracker: ServerSideTracker,
    private logger: Logger,
  ) {
    // messages with binary payloads are sent as two post calls. We first get the JSON message, then the binary payload,
    // which we will reconstruct together.
    let messageExpectingBinaryFollowup: ClientToServerMessageWithPayload | null = null;
    this.incomingListener = this.connection.onDidReceiveMessage((buf, isBinary) => {
      if (isBinary) {
        if (messageExpectingBinaryFollowup == null) {
          connection.logger?.error('Error: got a binary message when not expecting one');
          return;
        }
        // TODO: we don't handle queueing up messages with payloads...
        this.handleIncomingMessageWithPayload(messageExpectingBinaryFollowup, buf);
        messageExpectingBinaryFollowup = null;
        return;
      } else if (messageExpectingBinaryFollowup != null) {
        connection.logger?.error(
          'Error: didnt get binary payload after a message that requires one',
        );
        messageExpectingBinaryFollowup = null;
        return;
      }

      const message = buf.toString('utf-8');
      const data = deserializeFromString(message) as IncomingMessage;
      if (expectsBinaryPayload(data)) {
        // remember this message, and wait to get the binary payload before handling it
        messageExpectingBinaryFollowup = data;
        return;
      }

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

  private setCurrentRepo(repo: Repository, cwd: string) {
    this.disposeRepoDisposables();

    this.currentState = {type: 'repo', repo, cwd};

    this.tracker.context.setRepo(repo);

    if (repo.codeReviewProvider != null) {
      this.repoDisposables.push(
        repo.codeReviewProvider.onChangeDiffSummaries(value => {
          this.postMessage({type: 'fetchedDiffSummaries', summaries: value});
        }),
      );
    }

    this.repoDisposables.push(
      repo.subscribeToHeadCommit(head => {
        const allCommits = repo.getSmartlogCommits();
        const ancestor = findPublicAncestor(allCommits?.commits.value, head);
        this.tracker.track('HeadCommitChanged', {
          extras: {
            hash: head.hash,
            public: ancestor?.hash,
            bookmarks: ancestor?.remoteBookmarks,
          },
        });
      }),
    );

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
    this.activeRepoRef = repositoryCache.getOrCreate(command, this.logger, this.tracker, newCwd);
    this.activeRepoRef.promise.then(repoOrError => {
      if (repoOrError instanceof Repository) {
        this.setCurrentRepo(repoOrError, newCwd);
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

  private handleIncomingMessageWithPayload(
    message: IncomingMessageWithPayload,
    payload: ArrayBuffer,
  ) {
    switch (message.type) {
      case 'uploadFile': {
        const {id, filename} = message;
        const uploadFile = Internal.uploadFile;
        if (uploadFile == null) {
          return;
        }
        this.tracker
          .operation('UploadImage', 'UploadImageError', {}, () =>
            uploadFile(unwrap(this.connection.logger), {filename, data: payload}),
          )
          .then((result: string) => {
            this.connection.logger?.info('sucessfully uploaded file', filename, result);
            this.postMessage({type: 'uploadFileResult', id, result: {value: result}});
          })
          .catch((error: Error) => {
            this.connection.logger?.info('error uploading file', filename, error);
            this.postMessage({type: 'uploadFileResult', id, result: {error}});
          });
        break;
      }
    }
  }

  private handleIncomingMessage(data: IncomingMessage) {
    this.handleIncomingGeneralMessage(data as GeneralMessage);
    const {currentState} = this;
    switch (currentState.type) {
      case 'repo': {
        const {repo, cwd} = currentState;
        this.handleIncomingMessageWithRepo(data as WithRepoMessage, repo, cwd);
        break;
      }

      // If the repo is in the loading or error state, the client may still send
      // platform messages such as `platform/openExternal` that should be processed.
      case 'loading':
      case 'error':
        if (data.type.startsWith('platform/')) {
          this.platform.handleMessageFromClient(
            /*repo=*/ undefined,
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
      case 'track': {
        this.tracker.trackData(data.data);
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
              cwd: this.currentState.cwd,
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
        Internal.fileABug?.(
          data.data,
          data.uiState,
          this.tracker,
          this.logger,
          (progress: FileABugProgress) => {
            this.connection.logger?.info('file a bug progress: ', JSON.stringify(progress));
            this.postMessage({type: 'fileBugReportProgress', ...progress});
          },
        );
        break;
      }
    }
  }

  /**
   * Handle messages which require a repository to have been successfully set up to run
   */
  private handleIncomingMessageWithRepo(data: WithRepoMessage, repo: Repository, cwd: string) {
    const {logger} = repo;
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
        repo.runOrQueueOperation(
          operation,
          progress => {
            this.postMessage({type: 'operationProgress', ...progress});
            if (progress.kind === 'queue') {
              this.tracker.track('QueueOperation', {extras: {operation: operation.trackEventName}});
            }
          },
          this.tracker,
          cwd,
        );
        break;
      }
      case 'abortRunningOperation': {
        const {operationId} = data;
        repo.abortRunningOpeation(operationId);
        break;
      }
      case 'getConfig': {
        repo
          .getConfig(data.name)
          .catch(() => undefined)
          .then(value => {
            logger.info('got config', data.name, value);
            this.postMessage({type: 'gotConfig', name: data.name, value});
          });
        break;
      }
      case 'setConfig': {
        logger.info('set config', data.name, data.value);
        repo.setConfig('user', data.name, data.value).catch(err => {
          logger.error('error setting config', data.name, data.value, err);
        });
        break;
      }
      case 'requestComparison': {
        const {comparison} = data;
        const diff: Promise<Result<string>> = repo
          .runDiff(comparison)
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
          start,
          numLines,
        } = data;

        // TODO: For context lines, before/after sides of the comparison
        // are identical... except for line numbers.
        // Typical comparisons with '.' would be much faster (nearly instant)
        // by reading from the filesystem rather than using cat,
        // we just need the caller to ask with "after" line numbers instead of "before".
        // Note: we would still need to fall back to cat for comparisons that do not involve
        // the working copy.
        const cat: Promise<string> = repo
          .cat(relativePath, revsetForComparison(comparison))
          .catch(() => '');

        cat.then(content =>
          this.postMessage({
            type: 'comparisonContextLines',
            lines: content.split('\n').slice(start - 1, start - 1 + numLines),
            path: relativePath,
          }),
        );
        break;
      }
      case 'refresh': {
        logger?.log('refresh requested');
        repo.fetchSmartlogCommits();
        repo.fetchUncommittedChanges();
        repo.codeReviewProvider?.triggerDiffSummariesFetch(repo.getAllDiffIds());
        break;
      }
      case 'pageVisibility': {
        repo.setPageFocus(this.pageId, data.state);
        break;
      }
      case 'fetchCommitMessageTemplate': {
        this.handleFetchCommitMessageTemplate(repo);
        break;
      }
      case 'fetchShelvedChanges': {
        repo
          .getShelvedChanges()
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
          .lookupCommits([data.revset])
          .then(commits => {
            this.postMessage({
              type: 'fetchedLatestCommit',
              revset: data.revset,
              info: {value: commits.values().next().value},
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
      case 'fetchAllCommitChangedFiles': {
        repo
          .getAllChangedFiles(data.hash)
          .then(files => {
            this.postMessage({
              type: 'fetchedAllCommitChangedFiles',
              hash: data.hash,
              result: {value: files},
            });
          })
          .catch(err => {
            this.postMessage({
              type: 'fetchedAllCommitChangedFiles',
              hash: data.hash,
              result: {error: err as Error},
            });
          });
        break;
      }
      case 'fetchCommitCloudState': {
        repo.getCommitCloudState(cwd).then(state => {
          this.postMessage({
            type: 'fetchedCommitCloudState',
            state: {value: state},
          });
        });
        break;
      }
      case 'fetchGeneratedStatuses': {
        generatedFilesDetector
          .queryFilesGenerated(repo.logger, repo.info.repoRoot, data.paths)
          .then(results => {
            this.postMessage({type: 'fetchedGeneratedStatuses', results});
          });
        break;
      }
      case 'typeahead': {
        // Current repo's code review provider should be able to handle all
        // TypeaheadKinds for the fields in its defined schema.
        repo.codeReviewProvider?.typeahead?.(data.kind, data.query)?.then(result =>
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
          undefined,
          undefined,
          /* don't timeout */ 0,
          this.tracker,
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
          undefined,
          {stdin: stdinStream},
          /* don't timeout */ 0,
          this.tracker,
        );
        const reply = (imported?: ImportedStack, error?: string) => {
          this.postMessage({type: 'importedStack', imported: imported ?? [], error});
        };
        parseExecJson(exec, reply);
        break;
      }
      case 'fetchFeatureFlag': {
        Internal.fetchFeatureFlag?.(data.name).then((passes: boolean) => {
          this.logger.info(`feature flag ${data.name} ${passes ? 'PASSES' : 'FAILS'}`);
          this.postMessage({type: 'fetchedFeatureFlag', name: data.name, passes});
        });
        break;
      }
      case 'fetchInternalUserInfo': {
        Internal.fetchUserInfo?.().then((info: Serializable) => {
          this.logger.info('user info:', info);
          this.postMessage({type: 'fetchedInternalUserInfo', info});
        });
        break;
      }
      case 'generateAICommitMessage': {
        if (Internal.generateAICommitMessage == null) {
          break;
        }
        repo.runDiff(data.comparison, /* context lines */ 4).then(diff => {
          Internal.generateAICommitMessage?.(logger, {
            title: data.title,
            context: diff,
          })
            .catch((error: Error) => ({error}))
            .then((result: Result<string>) => {
              this.postMessage({
                type: 'generatedAICommitMessage',
                message: result,
                id: data.id,
              });
            });
        });
        break;
      }
      default: {
        if (repo.codeReviewProvider?.handleClientToServerMessage?.(data) === true) {
          break;
        }
        this.platform.handleMessageFromClient(
          repo,
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

  private async handleFetchCommitMessageTemplate(repo: Repository) {
    const {logger} = repo;
    try {
      const [result, customTemplate] = await Promise.all([
        repo.runCommand(
          ['debugcommitmessage', 'isl'],
          'FetchCommitTemplateCommand',
          undefined,
          undefined,
          undefined,
          this.tracker,
        ),
        Internal.getCustomDefaultCommitTemplate?.(repo),
      ]);

      let template = result.stdout
        .replace(repo.IGNORE_COMMIT_MESSAGE_LINES_REGEX, '')
        .replace(/^<Replace this line with a title. Use 1 line only, 67 chars or less>/, '');

      if (customTemplate?.trim() !== '') {
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
