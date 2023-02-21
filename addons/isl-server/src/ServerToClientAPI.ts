/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ClientConnection} from '.';
import type {Repository} from './Repository';
import type {ServerSideTracker} from './analytics/serverSideTracker';
import type {ServerPlatform} from './serverPlatform';
import type {
  ServerToClientMessage,
  ClientToServerMessage,
  Disposable,
  SmartlogCommits,
  SmartlogCommitsEvent,
  UncommittedChanges,
  UncommittedChangesEvent,
  Result,
  MergeConflicts,
  MergeConflictsEvent,
  RepositoryError,
  PlatformSpecificClientToServerMessages,
  FileABugProgress,
  ClientToServerMessageWithPayload,
} from 'isl/src/types';

import {Internal} from './Internal';
import {absolutePathForFileInRepo} from './Repository';
import fs from 'fs';
import {serializeToString, deserializeFromString} from 'isl/src/serialize';
import {revsetArgsForComparison, revsetForComparison} from 'shared/Comparison';
import {randomId, unwrap} from 'shared/utils';

export type IncomingMessage = ClientToServerMessage;
type IncomingMessageWithPayload = ClientToServerMessageWithPayload;
export type OutgoingMessage = ServerToClientMessage;

type GeneralMessage = IncomingMessage &
  (
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

  private queuedMessages: Array<IncomingMessage> = [];
  private currentState:
    | {type: 'loading'}
    | {type: 'repo'; repo: Repository; cwd: string}
    | {type: 'error'; error: RepositoryError} = {type: 'loading'};

  // React Dev mode means we subscribe+unsubscribe+resubscribe in the client,
  // causing multiple subscriptions on the server. To avoid that,
  // and for general robustness against duplicated work, we prevent
  // re-subscribing after the first subscription occurs.
  private hasSubscribedToSmartlogCommits = false;
  private hasSubscribedToUncommittedChanges = false;
  private hasSubscribedToMergeConflicts = false;

  private pageId = randomId();

  constructor(
    private platform: ServerPlatform,
    private connection: ClientConnection,
    private tracker: ServerSideTracker,
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

  setRepoError(error: RepositoryError) {
    this.disposeRepoDisposables();

    this.currentState = {type: 'error', error};

    this.tracker.context.setRepo(undefined);

    this.processQueuedMessages();
  }

  setCurrentRepo(repo: Repository, cwd: string) {
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

    this.processQueuedMessages();
  }

  postMessage(message: OutgoingMessage) {
    this.connection.postMessage(serializeToString(message));
  }

  dispose() {
    this.incomingListener.dispose();
    this.disposeRepoDisposables();
  }

  private disposeRepoDisposables() {
    this.repoDisposables.forEach(disposable => disposable.dispose());
    this.repoDisposables = [];
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
      case 'track': {
        this.tracker.trackData(data.data);
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
          platformName: this.platform.platformName,
          version: this.connection.version,
        });
        break;
      }
      case 'fileBugReport': {
        Internal.fileABug?.(
          data.data,
          data.uiState,
          this.tracker,
          this.connection.logFileLocation,
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
      case 'subscribeUncommittedChanges': {
        if (this.hasSubscribedToUncommittedChanges) {
          break;
        }
        this.hasSubscribedToUncommittedChanges = true;
        const {subscriptionID} = data;
        const postUncommittedChanges = (result: Result<UncommittedChanges>) => {
          const message: UncommittedChangesEvent = {
            type: 'uncommittedChanges',
            subscriptionID,
            files: result,
          };
          this.postMessage(message);
        };

        const uncommittedChanges = repo.getUncommittedChanges();
        if (uncommittedChanges != null) {
          postUncommittedChanges({value: uncommittedChanges});
        }

        // send changes as they come in from watchman
        this.repoDisposables.push(repo.subscribeToUncommittedChanges(postUncommittedChanges));
        // trigger a fetch on startup
        repo.fetchUncommittedChanges();

        this.repoDisposables.push(
          repo.subscribeToUncommittedChangesBeginFetching(() =>
            this.postMessage({type: 'beganFetchingUncommittedChangesEvent'}),
          ),
        );
        return;
      }
      case 'subscribeSmartlogCommits': {
        if (this.hasSubscribedToSmartlogCommits) {
          break;
        }
        this.hasSubscribedToSmartlogCommits = true;
        const {subscriptionID} = data;
        const postSmartlogCommits = (result: Result<SmartlogCommits>) => {
          const message: SmartlogCommitsEvent = {
            type: 'smartlogCommits',
            subscriptionID,
            commits: result,
          };
          this.postMessage(message);
        };

        const smartlogCommits = repo.getSmartlogCommits();
        if (smartlogCommits != null) {
          postSmartlogCommits({value: smartlogCommits});
        }
        // send changes as they come from file watcher
        this.repoDisposables.push(repo.subscribeToSmartlogCommitsChanges(postSmartlogCommits));
        // trigger a fetch on startup
        repo.fetchSmartlogCommits();

        this.repoDisposables.push(
          repo.subscribeToSmartlogCommitsBeginFetching(() =>
            this.postMessage({type: 'beganFetchingSmartlogCommitsEvent'}),
          ),
        );
        return;
      }
      case 'subscribeMergeConflicts': {
        if (this.hasSubscribedToMergeConflicts) {
          break;
        }
        this.hasSubscribedToMergeConflicts = true;
        const {subscriptionID} = data;
        const postMergeConflicts = (conflicts: MergeConflicts | undefined) => {
          const message: MergeConflictsEvent = {
            type: 'mergeConflicts',
            subscriptionID,
            conflicts,
          };
          this.postMessage(message);
        };

        const mergeConflicts = repo.getMergeConflicts();
        if (mergeConflicts != null) {
          postMergeConflicts(mergeConflicts);
        }

        this.repoDisposables.push(repo.onChangeConflictState(postMergeConflicts));
        return;
      }
      case 'runOperation': {
        const {operation} = data;
        repo.runOrQueueOperation(
          operation,
          progress => {
            this.postMessage({type: 'operationProgress', ...progress});
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
      case 'deleteFile': {
        const {filePath} = data;
        const absolutePath = absolutePathForFileInRepo(filePath, repo);
        // security: don't trust client messages to allow us to delete files outside the repository
        if (absolutePath == null) {
          logger.warn("can't delete file outside of the repo", filePath);
          return;
        }

        fs.promises
          .rm(absolutePath)
          .then(() => {
            logger.info('deleted file from filesystem', absolutePath);
          })
          .catch(err => {
            logger.error('unable to delete file', absolutePath, err);
          });
        break;
      }
      case 'requestComparison': {
        const {comparison} = data;
        const DIFF_CONTEXT_LINES = 4;
        const diff: Promise<Result<string>> = repo
          .runCommand([
            'diff',
            ...revsetArgsForComparison(comparison),
            // don't include a/ and b/ prefixes on files
            '--noprefix',
            '--no-binary',
            '--nodate',
            '--unified',
            String(DIFF_CONTEXT_LINES),
          ])
          .then(o => ({value: o.stdout}))
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
        repo
          .runCommand(['debugcommitmessage'])
          .then(result => {
            const template = result.stdout
              .replace(repo.IGNORE_COMMIT_MESSAGE_LINES_REGEX, '')
              .replace(/^<Replace this line with a title. Use 1 line only, 67 chars or less>/, '');
            this.postMessage({type: 'fetchedCommitMessageTemplate', template});
          })
          .catch(err => {
            logger?.error('Could not fetch commit message template', err);
          });
        break;
      }
      case 'fetchDiffSummaries': {
        repo.codeReviewProvider?.triggerDiffSummariesFetch(repo.getAllDiffIds());
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
      default: {
        this.platform.handleMessageFromClient(repo, data, message => this.postMessage(message));
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
}
