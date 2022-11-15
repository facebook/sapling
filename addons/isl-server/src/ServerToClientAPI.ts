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
} from 'isl/src/types';

import {browserServerPlatform} from './serverPlatform';
import {serializeToString, deserializeFromString} from 'isl/src/serialize';
import {revsetArgsForComparison, revsetForComparison} from 'shared/Comparison';
import {randomId, unwrap} from 'shared/utils';

export type IncomingMessage = ClientToServerMessage;
export type OutgoingMessage = ServerToClientMessage;

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
  private disposables: Array<Disposable> = [];
  private platform: ServerPlatform;

  private queuedMessages: Array<IncomingMessage> = [];
  private currentRepository: Repository | undefined = undefined;
  private currentRepoCwd: string | undefined = undefined;

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
      if (this.currentRepository == null) {
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

  setCurrentRepo(repo: Repository, cwd: string) {
    this.currentRepository = repo;
    this.currentRepoCwd = cwd;

    if (repo.codeReviewProvider != null) {
      this.disposables.push(
        repo.codeReviewProvider.onChangeDiffSummaries(value => {
          this.postMessage({type: 'fetchedDiffSummaries', summaries: value});
        }),
      );
    }

    for (const message of this.queuedMessages) {
      try {
        this.handleIncomingMessage(message);
      } catch (err) {
        repo.logger?.error('error handling queued message: ', message, err);
      }
    }
    this.queuedMessages = [];
  }

  private handleIncomingMessage(data: IncomingMessage) {
    // invariant: an initialized repository is attached by the time this is called
    const repo = unwrap(this.currentRepository);

    const {logger} = repo;

    switch (data.type) {
      case 'requestRepoInfo': {
        logger.log('repo info requested');
        this.postMessage({type: 'repoInfo', info: repo.info});
        break;
      }
      case 'subscribeUncommittedChanges': {
        if (this.hasSubscribedToUncommittedChanges) {
          return;
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
          return;
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
          return;
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
          unwrap(this.currentRepoCwd),
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
            repo.logger.error('error running diff', error.toString());
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
        logger.log('refresh requested');
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
            logger.error('Could not fetch commit message template', err);
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

    const listeners = this.listenersByType.get(data.type);
    if (!listeners) {
      return;
    }
    listeners.forEach(handle => handle(data));
  }

  postMessage(message: OutgoingMessage) {
    this.connection.postMessage(serializeToString(message));
  }

  dispose() {
    this.incomingListener.dispose();
  }
}
