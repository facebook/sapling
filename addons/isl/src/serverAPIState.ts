/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {MessageBusStatus} from './MessageBus';
import type {Operation} from './operations/Operation';
import type {
  ApplicationInfo,
  ChangedFile,
  CommitInfo,
  Hash,
  MergeConflicts,
  ProgressStep,
  RepoInfo,
  SmartlogCommits,
  SubscriptionKind,
  SubscriptionResultsData,
  UncommittedChanges,
} from './types';
import type {AtomEffect, CallbackInterface} from 'recoil';
import type {EnsureAssignedTogether} from 'shared/EnsureAssignedTogether';

import serverAPI from './ClientToServerAPI';
import messageBus from './MessageBus';
import {latestSuccessorsMap, successionTracker} from './SuccessionTracker';
import {Dag} from './dag/dag';
import {persistAtomToConfigEffect} from './persistAtomToConfigEffect';
import {clearOnCwdChange} from './recoilUtils';
import {initialParams} from './urlParams';
import {short} from './utils';
import {DEFAULT_DAYS_OF_COMMITS_TO_LOAD} from 'isl-server/src/constants';
import {selectorFamily, atom, DefaultValue, selector, useRecoilCallback} from 'recoil';
import {defer, randomId} from 'shared/utils';

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
      serverAPI.onSetup(() =>
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

export const applicationinfo = atom<ApplicationInfo | undefined>({
  key: 'applicationinfo',
  default: undefined,
  effects: [
    ({setSelf}) => {
      const disposable = serverAPI.onMessageOfType('applicationInfo', event => {
        setSelf(event.info);
      });
      return () => disposable.dispose();
    },
    () =>
      serverAPI.onSetup(() =>
        serverAPI.postMessage({
          type: 'requestApplicationInfo',
        }),
      ),
  ],
});

export const reconnectingStatus = atom<MessageBusStatus>({
  key: 'reconnectingStatus',
  default: {type: 'initializing'},
  effects: [
    ({setSelf}) => {
      const disposable = messageBus.onChangeStatus(setSelf);
      return () => disposable.dispose();
    },
  ],
});

export const serverCwd = selector<string>({
  key: 'serverCwd',
  get: ({get}) => {
    const data = get(repositoryData);
    if (data.info?.type === 'cwdNotARepository') {
      return data.info.cwd;
    }
    return data?.cwd ?? initialParams.get('cwd') ?? '';
  },
});

export async function forceFetchCommit(revset: string): Promise<CommitInfo> {
  serverAPI.postMessage({
    type: 'fetchLatestCommit',
    revset,
  });
  const response = await serverAPI.nextMessageMatching(
    'fetchedLatestCommit',
    message => message.revset === revset,
  );
  if (response.info.error) {
    throw response.info.error;
  }
  return response.info.value;
}

export const latestUncommittedChangesData = atom<{
  fetchStartTimestamp: number;
  fetchCompletedTimestamp: number;
  files: UncommittedChanges;
  error?: Error;
}>({
  key: 'latestUncommittedChangesData',
  default: {fetchStartTimestamp: 0, fetchCompletedTimestamp: 0, files: []},
  effects: [
    subscriptionEffect('uncommittedChanges', (data, {setSelf}) => {
      setSelf(last => ({
        ...data,
        files:
          data.files.value ??
          // leave existing files in place if there was no error
          (last instanceof DefaultValue ? [] : last.files) ??
          [],
        error: data.files.error,
      }));
    }),
  ],
});

/**
 * Latest fetched uncommitted file changes from the server, without any previews.
 * Prefer using `uncommittedChangesWithPreviews`, since it includes optimistic state
 * and previews.
 */
export const latestUncommittedChanges = selector<Array<ChangedFile>>({
  key: 'latestUncommittedChanges',
  get: ({get}) => {
    return get(latestUncommittedChangesData).files;
  },
});

export const uncommittedChangesFetchError = selector<Error | undefined>({
  key: 'uncommittedChangesFetchError',
  get: ({get}) => {
    return get(latestUncommittedChangesData).error;
  },
});

export const mergeConflicts = atom<MergeConflicts | undefined>({
  key: 'mergeConflicts',
  default: undefined,
  effects: [
    subscriptionEffect('mergeConflicts', (data, {setSelf}) => {
      setSelf(data);
    }),
  ],
});

export const latestCommitsData = atom<{
  fetchStartTimestamp: number;
  fetchCompletedTimestamp: number;
  commits: SmartlogCommits;
  error?: Error;
}>({
  key: 'latestCommitsData',
  default: {fetchStartTimestamp: 0, fetchCompletedTimestamp: 0, commits: []},
  effects: [
    subscriptionEffect('smartlogCommits', (data, {setSelf}) => {
      setSelf(last => ({
        ...data,
        commits:
          data.commits.value ??
          // leave existing files in place if there was no error
          (last instanceof DefaultValue ? [] : last.commits) ??
          [],
        error: data.commits.error,
      }));
      if (data.commits.value) {
        successionTracker.findNewSuccessionsFromCommits(data.commits.value);
      }
    }),
  ],
});

export const latestUncommittedChangesTimestamp = selector<number>({
  key: 'latestUncommittedChangesTimestamp',
  get: ({get}) => {
    return get(latestUncommittedChangesData).fetchCompletedTimestamp;
  },
});

/**
 * Lookup a commit by hash, *WITHOUT PREVIEWS*.
 * Generally, you'd want to look up WITH previews, which you can use dagWithPreviews for.
 */
export const commitByHash = selectorFamily<CommitInfo | undefined, string>({
  key: 'commitByHash',
  get:
    (hash: string) =>
    ({get}) => {
      return get(latestCommits).find(commit => commit.hash === hash);
    },
});

export const latestCommits = selector<Array<CommitInfo>>({
  key: 'latestCommits',
  get: ({get}) => {
    return get(latestCommitsData).commits;
  },
});

/** The dag also includes a mutationDag to answer successor queries. */
export const latestDag = selector<Dag>({
  key: 'latestDag',
  get: ({get}) => {
    const commits = get(latestCommits);
    const successorMap = get(latestSuccessorsMap);
    const commitDag = undefined; // will be populated from `commits`
    const dag = Dag.fromDag(commitDag, successorMap).add(commits).forceConnectPublic();
    return dag;
  },
});

export const commitFetchError = selector<Error | undefined>({
  key: 'commitFetchError',
  get: ({get}) => {
    return get(latestCommitsData).error;
  },
});

export const mostRecentSubscriptionIds: Record<SubscriptionKind, string> = {
  smartlogCommits: '',
  uncommittedChanges: '',
  mergeConflicts: '',
};

export const hasExperimentalFeatures = atom<boolean | null>({
  key: 'hasExperimentalFeatures',
  default: null,
  effects: [persistAtomToConfigEffect<boolean | null>('isl.experimental-features', false)],
});

/**
 * Send a subscribeFoo message to the server on initialization,
 * and send an unsubscribe message on dispose.
 * Extract subscription response messages via a unique subscriptionID per effect call.
 */
function subscriptionEffect<K extends SubscriptionKind, T>(
  kind: K,
  onData: (data: SubscriptionResultsData[K], params: Parameters<AtomEffect<T>>[0]) => unknown,
): AtomEffect<T> {
  return effectParams => {
    const subscriptionID = randomId();
    mostRecentSubscriptionIds[kind] = subscriptionID;
    const disposable = serverAPI.onMessageOfType('subscriptionResult', event => {
      if (event.subscriptionID !== subscriptionID || event.kind !== kind) {
        return;
      }
      onData(event.data as SubscriptionResultsData[K], effectParams);
    });

    const disposeSubscription = serverAPI.onSetup(() => {
      serverAPI.postMessage({
        type: 'subscribe',
        kind,
        subscriptionID,
      });

      return () =>
        serverAPI.postMessage({
          type: 'unsubscribe',
          kind,
          subscriptionID,
        });
    });

    return () => {
      disposable.dispose();
      disposeSubscription();
    };
  };
}

export const isFetchingCommits = atom<boolean>({
  key: 'isFetchingCommits',
  default: false,
  effects: [
    ({setSelf}) => {
      const disposables = [
        serverAPI.onMessageOfType('subscriptionResult', () => {
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

export const isFetchingAdditionalCommits = atom<boolean>({
  key: 'isFetchingAdditionalCommits',
  default: false,
  effects: [
    ({setSelf}) => {
      const disposables = [
        serverAPI.onMessageOfType('subscriptionResult', e => {
          if (e.kind === 'smartlogCommits') {
            setSelf(false);
          }
        }),
        serverAPI.onMessageOfType('beganLoadingMoreCommits', () => {
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
        serverAPI.onMessageOfType('subscriptionResult', e => {
          if (e.kind === 'uncommittedChanges') {
            setSelf(false); // new files OR error means the fetch is not running anymore
          }
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

export const commitsShownRange = atom<number | undefined>({
  key: 'commitsShownRange',
  default: DEFAULT_DAYS_OF_COMMITS_TO_LOAD,
  effects: [
    clearOnCwdChange(),
    ({setSelf}) => {
      const disposables = [
        serverAPI.onMessageOfType('commitsShownRange', event => {
          setSelf(event.rangeInDays);
        }),
      ];
      return () => {
        disposables.forEach(d => d.dispose());
      };
    },
  ],
});

/**
 * Latest head commit from original data from the server, without any previews.
 * Prefer using `dagWithPreviews.resolve('.')`, since it includes optimistic state
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
 * No longer in the "loading" state:
 * - Either the list of commits has successfully loaded
 * - or there was an error during the fetch
 */
export const haveCommitsLoadedYet = selector<boolean>({
  key: 'haveCommitsLoadedYet',
  get: ({get}) => {
    const data = get(latestCommitsData);
    return data.commits.length > 0 || data.error != null;
  },
});

export const operationBeingPreviewed = atom<Operation | undefined>({
  key: 'operationBeingPreviewed',
  default: undefined,
  effects: [clearOnCwdChange()],
});

export const haveRemotePath = selector({
  key: 'haveRemotePath',
  get: ({get}) => {
    const info = get(repositoryInfo);
    // codeReviewSystem.type is 'unknown' or other values if paths.default is present.
    return info?.type === 'success' && info.codeReviewSystem.type !== 'none';
  },
});

export type OperationInfo = {
  operation: Operation;
  startTime?: Date;
  commandOutput?: Array<string>;
  currentProgress?: ProgressStep;
  /** progress message shown next to a commit */
  inlineProgress?: Map<Hash, string>;
  /** if true, we have sent "abort" request, the process might have exited or is going to exit soon */
  aborting?: boolean;
  /** if true, the operation process has exited AND there's no more optimistic commit state to show */
  hasCompletedOptimisticState?: boolean;
  /** if true, the operation process has exited AND there's no more optimistic changes to uncommited changes to show */
  hasCompletedUncommittedChangesOptimisticState?: boolean;
  /** if true, the operation process has exited AND there's no more optimistic changes to merge conflicts to show */
  hasCompletedMergeConflictsOptimisticState?: boolean;
} & EnsureAssignedTogether<{
  endTime: Date;
  exitCode: number;
}>;

/**
 * Bundle history of previous operations together with the current operation,
 * so we can easily manipulate operations together in one piece of state.
 */
export interface OperationList {
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
    const inlineProgress: Array<[string, string]> | undefined = newOperation
      .getInitialInlineProgress?.()
      ?.map(([k, v]) => [short(k), v]); // inline progress is keyed by short hashes, but let's do that conversion on behalf of operations.
    const currentOperation: OperationInfo = {
      operation: newOperation,
      startTime: new Date(),
      inlineProgress: inlineProgress == null ? undefined : new Map(inlineProgress),
    };
    return {...list, operationHistory, currentOperation};
  }
}

export const operationList = atom<OperationList>({
  key: 'operationList',
  default: defaultOperationList(),
  effects: [
    clearOnCwdChange(),
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
                  currentProgress: undefined, // hide progress on new stdout, so it doesn't appear stuck
                },
              };
            });
            break;
          case 'inlineProgress':
            setSelf(current => {
              if (current == null || current instanceof DefaultValue) {
                return current;
              }
              const currentOperation = current.currentOperation;
              if (currentOperation == null) {
                return current;
              }

              let inlineProgress: undefined | Map<string, string> =
                current.currentOperation?.inlineProgress ?? new Map();
              if (progress.hash) {
                if (progress.message) {
                  inlineProgress.set(progress.hash, progress.message);
                } else {
                  inlineProgress.delete(progress.hash);
                }
              } else {
                inlineProgress = undefined;
              }

              const newCommandOutput = [...(currentOperation?.commandOutput ?? [])];
              if (progress.hash && progress.message) {
                // also add inline progress message as if it was on stdout,
                // so you can see it when reading back the final output
                newCommandOutput.push(`${progress.hash} - ${progress.message}\n`);
              }

              return {
                ...current,
                currentOperation: {
                  ...currentOperation,
                  inlineProgress,
                },
              };
            });
            break;
          case 'progress':
            setSelf(current => {
              if (current == null || current instanceof DefaultValue) {
                return current;
              }
              const currentOperation = current.currentOperation;
              if (currentOperation == null) {
                return current;
              }

              const newCommandOutput = [...(currentOperation?.commandOutput ?? [])];
              if (newCommandOutput.at(-1) !== progress.progress.message) {
                // also add progress message as if it was on stdout,
                // so you can see it when reading back the final output,
                // but only if it's a different progress message than we've seen.
                newCommandOutput.push(progress.progress.message + '\n');
              }

              return {
                ...current,
                currentOperation: {
                  ...currentOperation,
                  commandOutput: newCommandOutput,
                  currentProgress: progress.progress,
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

              const complete = operationCompletionCallbacks.get(currentOperation.operation.id);
              complete?.();
              operationCompletionCallbacks.delete(currentOperation.operation.id);

              return {
                ...current,
                currentOperation: {
                  ...currentOperation,
                  exitCode: progress.exitCode,
                  endTime: new Date(progress.timestamp),
                  inlineProgress: undefined, // inline progress never lasts after exiting
                },
              };
            });
            break;
        }
      });
      return () => disposable.dispose();
    },
  ],
});

export const inlineProgressByHash = selectorFamily<string | undefined, string>({
  key: 'inlineProgressByHash',
  get:
    (hash: string) =>
    ({get}) => {
      const info = get(operationList);
      const inlineProgress = info.currentOperation?.inlineProgress;
      if (inlineProgress == null) {
        return undefined;
      }
      const shortHash = short(hash); // progress messages come indexed by short hash
      return inlineProgress.get(shortHash);
    },
});

/** We don't send entire operations to the server, since not all fields are serializable.
 * Thus, when the server tells us about the queue of operations, we need to know which operation it's talking about.
 * Store recently run operations by id. Add to this map whenever a new operation is run. Remove when an operation process exits (successfully or unsuccessfully)
 */
const operationsById = new Map<string, Operation>();
/** Store callbacks to run when an operation completes. This is stored outside of the operation since Operations are typically Immutable. */
const operationCompletionCallbacks = new Map<string, () => void>();

export const queuedOperations = atom<Array<Operation>>({
  key: 'queuedOperations',
  default: [],
  effects: [
    clearOnCwdChange(),
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
): Promise<void> {
  // TODO: check for hashes in arguments that are known to be obsolete already,
  // and mark those to not be rewritten.
  serverAPI.postMessage({
    type: 'runOperation',
    operation: {
      args: operation.getArgs(),
      id: operation.id,
      stdin: operation.getStdin(),
      runner: operation.runner,
      trackEventName: operation.trackEventName,
    },
  });
  const defered = defer<void>();
  operationCompletionCallbacks.set(operation.id, () => defered.resolve());

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

  return defered.promise;
}

/**
 * Returns callback to run an operation.
 * Will be queued by the server if other operations are already running.
 * This returns a promise that resolves when this operation has exited
 * (though its optimistic state may not have finished resolving yet).
 * Note: There's no need to wait for this promise to resolve before starting another operation,
 * successive operations will queue up with a nicer UX than if you awaited each one.
 */
export function useRunOperation() {
  return useRecoilCallback(
    ({snapshot, set}) =>
      (operation: Operation): Promise<void> => {
        return runOperationImpl(snapshot, set, operation);
      },
    [],
  );
}

/**
 * Returns callback to abort the running operation.
 */
export function useAbortRunningOperation() {
  return useRecoilCallback(({snapshot, set}) => (operationId: string) => {
    serverAPI.postMessage({
      type: 'abortRunningOperation',
      operationId,
    });
    const ongoing = snapshot.getLoadable(operationList).valueMaybe();
    if (ongoing?.currentOperation?.operation?.id === operationId) {
      // Mark 'aborting' as true.
      set(operationList, list => {
        const currentOperation = list.currentOperation;
        if (currentOperation != null) {
          return {...list, currentOperation: {aborting: true, ...currentOperation}};
        }
        return list;
      });
    }
  });
}

/**
 * Returns callback to run the operation currently being previewed, or cancel the preview.
 * Set operationBeingPreviewed to start a preview.
 */
export function useRunPreviewedOperation() {
  return useRecoilCallback(
    ({snapshot, set}) =>
      (isCancel: boolean, operation?: Operation) => {
        if (isCancel) {
          set(operationBeingPreviewed, undefined);
          return;
        }

        const operationToRun =
          operation ?? snapshot.getLoadable(operationBeingPreviewed).valueMaybe();
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
              if (!operationHistory[i].hasCompletedMergeConflictsOptimisticState) {
                operationHistory[i] = {
                  ...operationHistory[i],
                  hasCompletedMergeConflictsOptimisticState: true,
                };
              }
            }
          }
          const currentOperation =
            list.currentOperation == null ? undefined : {...list.currentOperation};
          if (currentOperation?.exitCode != null) {
            currentOperation.hasCompletedOptimisticState = true;
            currentOperation.hasCompletedUncommittedChangesOptimisticState = true;
            currentOperation.hasCompletedMergeConflictsOptimisticState = true;
          }
          return {currentOperation, operationHistory};
        });
      },
    [],
  );
}
