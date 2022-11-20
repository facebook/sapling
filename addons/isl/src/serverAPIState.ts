/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {EditedMessage} from './CommitInfo';
import type {CommitTree} from './getCommitTree';
import type {Operation} from './operations/Operation';
import type {ChangedFile, CommitInfo, Hash, MergeConflicts, RepoInfo} from './types';
import type {CallbackInterface} from 'recoil';

import serverAPI from './ClientToServerAPI';
import {getCommitTree, walkTreePostorder} from './getCommitTree';
import {operationBeingPreviewed} from './previews';
import {initialParams} from './urlParams';
import {firstLine} from './utils';
import {atom, DefaultValue, selector, useRecoilCallback} from 'recoil';

const repositoryData = atom<{info: RepoInfo | undefined; cwd: string | undefined}>({
  key: 'repositoryData',
  default: {info: undefined, cwd: undefined},
  effects: [
    ({setSelf}) => {
      const disposable = serverAPI.onMessageOfType('repoInfo', event => {
        setSelf({info: event.info, cwd: event.cwd});
      });
      return () => disposable.dispose();
    },
    () =>
      serverAPI.onConnectOrReconnect(() =>
        serverAPI.postMessage({
          type: 'requestRepoInfo',
        }),
      ),
  ],
});

export const repositoryInfo = selector<RepoInfo | undefined>({
  key: 'repositoryInfo',
  get: ({get}) => {
    const data = get(repositoryData);
    return data?.info;
  },
  set: ({set}, value) => {
    set(repositoryData, last => ({
      ...last,
      info: value instanceof DefaultValue ? undefined : value,
    }));
  },
});

export const serverCwd = selector<string>({
  key: 'serverCwd',
  get: ({get}) => {
    const data = get(repositoryData);
    return data?.cwd ?? initialParams.get('cwd') ?? '';
  },
});

/**
 * Latest fetched uncommitted file changes from the server, without any previews.
 * Prefer using `uncommittedChangesWithPreviews`, since it includes optimistic state
 * and previews.
 */
export const latestUncommittedChanges = atom<Array<ChangedFile>>({
  key: 'latestUncommittedChanges',
  default: [],
  effects: [
    ({setSelf}) => {
      const disposable = serverAPI.onMessageOfType('uncommittedChanges', event => {
        if (event.files.error) {
          // leave existing file changes in place
          return;
        }
        setSelf(event.files.value);
      });
      return () => disposable.dispose();
    },
    () =>
      serverAPI.onConnectOrReconnect(() =>
        serverAPI.postMessage({
          type: 'subscribeUncommittedChanges',
          subscriptionID: 'latestUncommittedChanges',
        }),
      ),
  ],
});

export const mergeConflicts = atom<MergeConflicts | undefined>({
  key: 'mergeConflicts',
  default: undefined,
  effects: [
    ({setSelf}) => {
      const disposable = serverAPI.onMessageOfType('mergeConflicts', event => {
        setSelf(event.conflicts);
      });
      return () => disposable.dispose();
    },
    () =>
      serverAPI.onConnectOrReconnect(() =>
        serverAPI.postMessage({
          type: 'subscribeMergeConflicts',
          subscriptionID: 'latestMergeConflicts',
        }),
      ),
  ],
});

export const latestCommits = atom<Array<CommitInfo>>({
  key: 'latestCommits',
  default: [],
  effects: [
    ({setSelf}) => {
      const disposable = serverAPI.onMessageOfType('smartlogCommits', event => {
        if (event.commits.error) {
          // leave existing commits in place
          return;
        }
        setSelf(event.commits.value);
      });
      return () => disposable.dispose();
    },
    () =>
      serverAPI.onConnectOrReconnect(() =>
        serverAPI.postMessage({
          type: 'subscribeSmartlogCommits',
          subscriptionID: 'latestCommits',
        }),
      ),
  ],
});

export const isFetchingCommits = atom<boolean>({
  key: 'isFetchingCommits',
  default: false,
  effects: [
    ({setSelf}) => {
      const disposables = [
        serverAPI.onMessageOfType('smartlogCommits', () => {
          setSelf(false); // new commits OR error means the fetch is not running anymore
        }),
        serverAPI.onMessageOfType('beganFetchingSmartlogCommitsEvent', () => {
          setSelf(true);
        }),
      ];
      return () => {
        disposables.forEach(d => d.dispose());
      };
    },
  ],
});

export const isFetchingUncommittedChanges = atom<boolean>({
  key: 'isFetchingUncommittedChanges',
  default: false,
  effects: [
    ({setSelf}) => {
      const disposables = [
        serverAPI.onMessageOfType('uncommittedChanges', () => {
          setSelf(false); // new files OR error means the fetch is not running anymore
        }),
        serverAPI.onMessageOfType('beganFetchingUncommittedChangesEvent', () => {
          setSelf(true);
        }),
      ];
      return () => {
        disposables.forEach(d => d.dispose());
      };
    },
  ],
});

export const commitFetchError = atom<Error | undefined>({
  key: 'commitFetchError',
  default: undefined,
  effects: [
    ({setSelf}) => {
      const disposable = serverAPI.onMessageOfType('smartlogCommits', event => {
        setSelf(event.commits.error); // set even if error is undefined as a way of clearing the error
      });
      return () => disposable.dispose();
    },
  ],
});

export const uncommittedChangesFetchError = atom<Error | undefined>({
  key: 'uncommittedChangesFetchError',
  default: undefined,
  effects: [
    ({setSelf}) => {
      const disposable = serverAPI.onMessageOfType('uncommittedChanges', event => {
        setSelf(event.files.error); // set even if error is undefined as a way of clearing the error
      });
      return () => disposable.dispose();
    },
  ],
});

export const commitMessageTemplate = atom<EditedMessage | undefined>({
  key: 'commitMessageTemplate',
  default: undefined,
  effects: [
    ({setSelf}) => {
      const disposable = serverAPI.onMessageOfType('fetchedCommitMessageTemplate', event => {
        const title = firstLine(event.template);
        const description = event.template.slice(title.length + 1);
        setSelf({title, description});
      });
      return () => disposable.dispose();
    },
    () =>
      serverAPI.onConnectOrReconnect(() =>
        serverAPI.postMessage({
          type: 'fetchCommitMessageTemplate',
        }),
      ),
  ],
});

/**
 * Latest fetched commit tree from the server, without any previews.
 * Prefer using `treeWithPreviews.trees`, since it includes optimistic state
 * and previews.
 */
export const latestCommitTree = selector<Array<CommitTree>>({
  key: 'latestCommitTree',
  get: ({get}) => {
    const commits = get(latestCommits);
    const tree = getCommitTree(commits);
    return tree;
  },
});

/**
 * Latest head commit from original data from the server, without any previews.
 * Prefer using `treeWithPreviews.headCommit`, since it includes optimistic state
 * and previews.
 */
export const latestHeadCommit = selector<CommitInfo | undefined>({
  key: 'latestHeadCommit',
  get: ({get}) => {
    const commits = get(latestCommits);
    return commits.find(commit => commit.isHead);
  },
});

/**
 * Mapping of commit hash -> subtree at that commit
 * Latest mapping of commit hash -> subtree at that commit from original data
 * from the server, without any previews.
 * Prefer using `treeWithPreviews.treeMap`, since it includes
 * optimistic state and previews.
 */
export const latestCommitTreeMap = selector<Map<Hash, CommitTree>>({
  key: 'latestCommitTreeMap',
  get: ({get}) => {
    const trees = get(latestCommitTree);
    const map = new Map();
    for (const tree of walkTreePostorder(trees)) {
      map.set(tree.info.hash, tree);
    }

    return map;
  },
});

export const haveCommitsLoadedYet = selector<boolean>({
  key: 'haveCommitsLoadedYet',
  get: ({get}) => {
    const commits = get(latestCommits);
    return commits.length > 0;
  },
});

export type OperationInfo = {
  operation: Operation;
  startTime?: Date;
  endTime?: Date;
  commandOutput?: Array<string>;
  exitCode?: number | null;
  /** if true, the operation process has exited AND there's no more optimistic commit state to show */
  hasCompletedOptimisticState?: boolean;
  /** if true, the operation process has exited AND there's no more optimistic changes to uncommited changes to show */
  hasCompletedUncommittedChangesOptimisticState?: boolean;
};

/**
 * Bundle history of previous operations together with the current operation,
 * so we can easily manipulate operations together in one piece of state.
 */
interface OperationList {
  /** The currently running operation, or the most recently run if not currently running. */
  currentOperation: OperationInfo | undefined;
  /** All previous operations oldest to newest, not including currentOperation */
  operationHistory: Array<OperationInfo>;
}
const defaultOperationList = () => ({currentOperation: undefined, operationHistory: []});

function startNewOperation(newOperation: Operation, list: OperationList): OperationList {
  if (list.currentOperation?.operation.id === newOperation.id) {
    // we already have a new optimistic running operation, don't duplicate it
    return {...list};
  } else {
    // we need to start a new operation
    const operationHistory = [...list.operationHistory];
    if (list.currentOperation != null) {
      operationHistory.push(list.currentOperation);
    }
    const currentOperation = {operation: newOperation};
    return {...list, operationHistory, currentOperation};
  }
}

export const operationList = atom<OperationList>({
  key: 'operationList',
  default: defaultOperationList(),
  effects: [
    ({setSelf}) => {
      const disposable = serverAPI.onMessageOfType('operationProgress', progress => {
        switch (progress.kind) {
          case 'spawn':
            setSelf(current => {
              const list = current instanceof DefaultValue ? defaultOperationList() : current;
              const operation = operationsById.get(progress.id);
              if (operation == null) {
                return current;
              }

              return startNewOperation(operation, list);
            });
            break;
          case 'stdout':
          case 'stderr':
            setSelf(current => {
              if (current == null || current instanceof DefaultValue) {
                return current;
              }
              const currentOperation = current.currentOperation;
              if (currentOperation == null) {
                return current;
              }

              return {
                ...current,
                currentOperation: {
                  ...currentOperation,
                  commandOutput: [...(currentOperation?.commandOutput ?? []), progress.message],
                },
              };
            });
            break;
          case 'exit':
            setSelf(current => {
              if (current == null || current instanceof DefaultValue) {
                return current;
              }
              const currentOperation = current.currentOperation;
              if (currentOperation == null) {
                return current;
              }

              return {
                ...current,
                currentOperation: {...currentOperation, exitCode: progress.exitCode},
              };
            });
            break;
        }
      });
      return () => disposable.dispose();
    },
  ],
});

// We don't send entire operations to the server, since not all fields are serializable.
// Thus, when the server tells us about the queue of operations, we need to know which operation it's talking about.
// Store recently run operations by id. Add to this map whenever a new operation is run. Remove when an operation process exits (successfully or unsuccessfully)
const operationsById = new Map<string, Operation>();

export const queuedOperations = atom<Array<Operation>>({
  key: 'queuedOperations',
  default: [],
  effects: [
    ({setSelf}) => {
      const disposable = serverAPI.onMessageOfType('operationProgress', progress => {
        switch (progress.kind) {
          case 'queue':
          case 'spawn': // spawning doubles as our notification to dequeue the next operation, and includes the new queue state.
            // Update with the latest queue state. We expect this to be sent whenever we try to run a command but it gets queued.
            setSelf(() => {
              return progress.queue
                .map(opId => operationsById.get(opId))
                .filter((op): op is Operation => op != null);
            });
            break;
          case 'error':
            setSelf(() => []); // empty queue when a command hits an error
            break;
          case 'exit':
            setSelf(current => {
              operationsById.delete(progress.id); // we don't need to care about this operation anymore
              if (progress.exitCode != null && progress.exitCode !== 0) {
                // if any process in the queue exits with an error, the entire queue is cleared.
                return [];
              }
              return current;
            });
            break;
        }
      });
      return () => disposable.dispose();
    },
  ],
});

function runOperationImpl(
  snapshot: CallbackInterface['snapshot'],
  set: CallbackInterface['set'],
  operation: Operation,
) {
  // TODO: check for hashes in arguments that are known to be obsolete already,
  // and mark those to not be rewritten.
  serverAPI.postMessage({
    type: 'runOperation',
    operation: {args: operation.getArgs(), id: operation.id, runner: operation.runner},
  });
  operationsById.set(operation.id, operation);
  const ongoing = snapshot.getLoadable(operationList).valueMaybe();
  if (ongoing?.currentOperation != null && ongoing.currentOperation.exitCode == null) {
    const queue = snapshot.getLoadable(queuedOperations).valueMaybe();
    // Add to the queue optimistically. The server will tell us the real state of the queue when it gets our run request.
    set(queuedOperations, [...(queue || []), operation]);
  } else {
    // start a new operation. We need to manage the previous operations
    set(operationList, list => startNewOperation(operation, list));
  }
}

/**
 * Returns callback to run an operation.
 * Will be queued by the server if other operations are already running.
 */
export function useRunOperation() {
  return useRecoilCallback(
    ({snapshot, set}) =>
      (operation: Operation) => {
        runOperationImpl(snapshot, set, operation);
      },
    [],
  );
}

/**
 * Returns callback to run the operation currently being previewed, or cancel the preview.
 * Set operationBeingPreviewed to start a preview.
 */
export function useRunPreviewedOperation() {
  return useRecoilCallback(
    ({snapshot, set}) =>
      (isCancel: boolean) => {
        if (isCancel) {
          set(operationBeingPreviewed, undefined);
          return;
        }

        const operationToRun = snapshot.getLoadable(operationBeingPreviewed).valueMaybe();
        set(operationBeingPreviewed, undefined);
        if (operationToRun) {
          runOperationImpl(snapshot, set, operationToRun);
        }
      },
    [],
  );
}

/**
 * It's possible for optimistic state to be incorrect, e.g. if some assumption about a command is incorrect in an edge case
 * but the command doesn't exit non-zero. This provides a backdoor to clear out all ongoing optimistic state from *previous* commands.
 * Queued commands and the currently running command will not be affected.
 */
export function useClearAllOptimisticState() {
  return useRecoilCallback(
    ({set}) =>
      () => {
        set(operationList, list => {
          const operationHistory = [...list.operationHistory];
          for (let i = 0; i < operationHistory.length; i++) {
            if (operationHistory[i].exitCode != null) {
              if (!operationHistory[i].hasCompletedOptimisticState) {
                operationHistory[i] = {...operationHistory[i], hasCompletedOptimisticState: true};
              }
              if (!operationHistory[i].hasCompletedUncommittedChangesOptimisticState) {
                operationHistory[i] = {
                  ...operationHistory[i],
                  hasCompletedUncommittedChangesOptimisticState: true,
                };
              }
            }
          }
          const currentOperation =
            list.currentOperation == null ? undefined : {...list.currentOperation};
          if (currentOperation?.exitCode != null) {
            currentOperation.hasCompletedOptimisticState = true;
            currentOperation.hasCompletedUncommittedChangesOptimisticState = true;
          }
          return {currentOperation, operationHistory};
        });
      },
    [],
  );
}
