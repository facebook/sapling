/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ClientConnection} from '.';
import type {Repository} from './Repository';
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
} from 'isl/src/types';

import {browserServerPlatform} from './serverPlatform';
import {serializeToString, deserializeFromString} from 'isl/src/serialize';
import {revsetArgsForComparison, revsetForComparison} from 'shared/Comparison';
import {randomId} from 'shared/utils';

export type IncomingMessage = ClientToServerMessage;
export type OutgoingMessage = ServerToClientMessage;

type GeneralMessage = IncomingMessage & {type: 'requestRepoInfo'};
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
  private platform: ServerPlatform;

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

  constructor(private connection: ClientConnection) {
    this.platform = connection.platform ?? browserServerPlatform;
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

  setRepoError(error: RepositoryError) {
    this.disposeRepoDisposables();

    this.currentState = {type: 'error', error};

    this.processQueuedMessages();
  }

  setCurrentRepo(repo: Repository, cwd: string) {
    this.disposeRepoDisposables();

    this.currentState = {type: 'repo', repo, cwd};

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
        repo.subscribeToUncommittedChanges(postUncommittedChanges);
        // trigger a fetch on startup
        repo.fetchUncommittedChanges();

        repo.subscribeToUncommittedChangesBeginFetching(() =>
          this.postMessage({type: 'beganFetchingUncommittedChangesEvent'}),
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
        repo.subscribeToSmartlogCommitsChanges(postSmartlogCommits);
        // trigger a fetch on startup
        repo.fetchSmartlogCommits();

        repo.subscribeToSmartlogCommitsBeginFetching(() =>
          this.postMessage({type: 'beganFetchingSmartlogCommitsEvent'}),
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

        repo.onChangeConflictState(postMergeConflicts);
        return;
      }
      case 'runOperation': {
        const {operation} = data;
        repo.runOrQueueOperation(
          operation,
          progress => {
            this.postMessage({type: 'operationProgress', ...progress});
          },
          cwd,
        );
        break;
      }
      case 'requestComparison': {
        const {comparison} = data;
        const DIFF_CONTEXT_LINES = 4;
        const diff: Promise<Result<string>> = repo
          .runCommand([
            'diff',
            ...revsetArgsForComparison(comparison),
            // git comparison mode presents renames in a more compact format that
            // our diff parsing client can't understand.
            '--no-git',
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
