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
import type {EnsureAssignedTogether} from 'shared/EnsureAssignedTogether';

import serverAPI from './ClientToServerAPI';
import messageBus from './MessageBus';
import {latestSuccessorsMapAtom, successionTracker} from './SuccessionTracker';
import {Dag, DagCommitInfo} from './dag/dag';
import {readInterestingAtoms, serializeAtomsState} from './debug/getInterestingAtoms';
import {atomFamilyWeak, configBackedAtom, readAtom, writeAtom} from './jotaiUtils';
import {atomResetOnCwdChange, repositoryData} from './repositoryData';
import {Timer} from './timer';
import {registerCleanup, registerDisposable, short} from './utils';
import {DEFAULT_DAYS_OF_COMMITS_TO_LOAD} from 'isl-server/src/constants';
import {atom} from 'jotai';
import {useCallback} from 'react';
import {reuseEqualObjects} from 'shared/deepEqualExt';
import {defer, randomId} from 'shared/utils';

export {repositoryData};

registerDisposable(
  repositoryData,
  serverAPI.onMessageOfType('repoInfo', event => {
    writeAtom(repositoryData, {info: event.info, cwd: event.cwd});
  }),
  import.meta.hot,
);
registerCleanup(
  repositoryData,
  serverAPI.onSetup(() =>
    serverAPI.postMessage({
      type: 'requestRepoInfo',
    }),
  ),
  import.meta.hot,
);

export const repositoryInfo = atom(
  get => {
    const data = get(repositoryData);
    return data?.info;
  },
  (
    get,
    set,
    update: RepoInfo | undefined | ((_prev: RepoInfo | undefined) => RepoInfo | undefined),
  ) => {
    const value = typeof update === 'function' ? update(get(repositoryData)?.info) : update;
    set(repositoryData, last => ({
      ...last,
      info: value,
    }));
  },
);

export const applicationinfo = atom<ApplicationInfo | undefined>(undefined);
registerDisposable(
  applicationinfo,
  serverAPI.onMessageOfType('applicationInfo', event => {
    writeAtom(applicationinfo, event.info);
  }),
  import.meta.hot,
);
registerCleanup(
  applicationinfo,
  serverAPI.onSetup(() =>
    serverAPI.postMessage({
      type: 'requestApplicationInfo',
    }),
  ),
  import.meta.hot,
);

export const reconnectingStatus = atom<MessageBusStatus>({type: 'initializing'});
registerDisposable(
  reconnectingStatus,
  messageBus.onChangeStatus(status => {
    writeAtom(reconnectingStatus, status);
  }),
  import.meta.hot,
);

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

export const mostRecentSubscriptionIds: Record<SubscriptionKind, string> = {
  smartlogCommits: '',
  uncommittedChanges: '',
  mergeConflicts: '',
};

/**
 * Send a subscribeFoo message to the server on initialization,
 * and send an unsubscribe message on dispose.
 * Extract subscription response messages via a unique subscriptionID per effect call.
 */
function subscriptionEffect<K extends SubscriptionKind>(
  kind: K,
  onData: (data: SubscriptionResultsData[K]) => unknown,
): () => void {
  const subscriptionID = randomId();
  mostRecentSubscriptionIds[kind] = subscriptionID;
  const disposable = serverAPI.onMessageOfType('subscriptionResult', event => {
    if (event.subscriptionID !== subscriptionID || event.kind !== kind) {
      return;
    }
    onData(event.data as SubscriptionResultsData[K]);
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
}

export const latestUncommittedChangesData = atom<{
  fetchStartTimestamp: number;
  fetchCompletedTimestamp: number;
  files: UncommittedChanges;
  error?: Error;
}>({fetchStartTimestamp: 0, fetchCompletedTimestamp: 0, files: []});
// This is used by a test. Tests do not go through babel to rewrite source
// to insert debugLabel.
latestUncommittedChangesData.debugLabel = 'latestUncommittedChangesData';

registerCleanup(
  latestUncommittedChangesData,
  subscriptionEffect('uncommittedChanges', data => {
    writeAtom(latestUncommittedChangesData, last => ({
      ...data,
      files:
        data.files.value ??
        // leave existing files in place if there was no error
        (last.error == null ? [] : last.files) ??
        [],
      error: data.files.error,
    }));
  }),
  import.meta.hot,
);

/**
 * Latest fetched uncommitted file changes from the server, without any previews.
 * Prefer using `uncommittedChangesWithPreviews`, since it includes optimistic state
 * and previews.
 */
export const latestUncommittedChanges = atom<Array<ChangedFile>>(
  get => get(latestUncommittedChangesData).files,
);

export const uncommittedChangesFetchError = atom(get => {
  return get(latestUncommittedChangesData).error;
});

export const mergeConflicts = atom<MergeConflicts | undefined>(undefined);
registerCleanup(
  mergeConflicts,
  subscriptionEffect('mergeConflicts', data => {
    writeAtom(mergeConflicts, data);
  }),
);

export const latestCommitsData = atom<{
  fetchStartTimestamp: number;
  fetchCompletedTimestamp: number;
  commits: SmartlogCommits;
  error?: Error;
}>({fetchStartTimestamp: 0, fetchCompletedTimestamp: 0, commits: []});

registerCleanup(
  latestCommitsData,
  subscriptionEffect('smartlogCommits', data => {
    writeAtom(latestCommitsData, last => {
      let commits = last.commits;
      const newCommits = data.commits.value;
      if (newCommits != null) {
        // leave existing commits in place if there was no erro
        commits = reuseEqualObjects(commits, newCommits, c => c.hash);
      }
      return {
        ...data,
        commits,
        error: data.commits.error,
      };
    });
    if (data.commits.value) {
      successionTracker.findNewSuccessionsFromCommits(data.commits.value);
    }
  }),
);

export const latestUncommittedChangesTimestamp = atom(get => {
  return get(latestUncommittedChangesData).fetchCompletedTimestamp;
});

/**
 * Lookup a commit by hash, *WITHOUT PREVIEWS*.
 * Generally, you'd want to look up WITH previews, which you can use dagWithPreviews for.
 */
export const commitByHash = atomFamilyWeak((hash: string) => atom(get => get(latestDag).get(hash)));

export const latestCommits = atom(get => {
  return get(latestCommitsData).commits;
});

/** The dag also includes a mutationDag to answer successor queries. */
export const latestDag = atom(get => {
  const commits = get(latestCommits);
  const successorMap = get(latestSuccessorsMapAtom);
  const commitDag = undefined; // will be populated from `commits`
  const dag = Dag.fromDag(commitDag, successorMap)
    .add(commits.map(c => DagCommitInfo.fromCommitInfo(c)))
    .forceConnectPublic();
  return dag;
});

export const commitFetchError = atom(get => {
  return get(latestCommitsData).error;
});

export const hasExperimentalFeatures = configBackedAtom<boolean | null>(
  'isl.experimental-features',
  false,
  true /* read-only */,
);

export const isFetchingCommits = atom(false);
registerDisposable(
  isFetchingCommits,
  serverAPI.onMessageOfType('subscriptionResult', () => {
    writeAtom(isFetchingCommits, false); // new commits OR error means the fetch is not running anymore
  }),
  import.meta.hot,
);
registerDisposable(
  isFetchingCommits,
  serverAPI.onMessageOfType('beganFetchingSmartlogCommitsEvent', () => {
    writeAtom(isFetchingCommits, true);
  }),
  import.meta.hot,
);

export const isFetchingAdditionalCommits = atom(false);
registerDisposable(
  isFetchingAdditionalCommits,
  serverAPI.onMessageOfType('subscriptionResult', e => {
    if (e.kind === 'smartlogCommits') {
      writeAtom(isFetchingAdditionalCommits, false);
    }
  }),
  import.meta.hot,
);
registerDisposable(
  isFetchingAdditionalCommits,
  serverAPI.onMessageOfType('subscriptionResult', e => {
    if (e.kind === 'smartlogCommits') {
      writeAtom(isFetchingAdditionalCommits, false);
    }
  }),
  import.meta.hot,
);
registerDisposable(
  isFetchingAdditionalCommits,
  serverAPI.onMessageOfType('beganLoadingMoreCommits', () => {
    writeAtom(isFetchingAdditionalCommits, true);
  }),
  import.meta.hot,
);

export const isFetchingUncommittedChanges = atom(false);
registerDisposable(
  isFetchingUncommittedChanges,
  serverAPI.onMessageOfType('subscriptionResult', e => {
    if (e.kind === 'uncommittedChanges') {
      writeAtom(isFetchingUncommittedChanges, false); // new files OR error means the fetch is not running anymore
    }
  }),
  import.meta.hot,
);
registerDisposable(
  isFetchingUncommittedChanges,
  serverAPI.onMessageOfType('beganFetchingUncommittedChangesEvent', () => {
    writeAtom(isFetchingUncommittedChanges, true);
  }),
  import.meta.hot,
);

export const commitsShownRange = atomResetOnCwdChange<number | undefined>(
  DEFAULT_DAYS_OF_COMMITS_TO_LOAD,
);
registerDisposable(
  applicationinfo,
  serverAPI.onMessageOfType('commitsShownRange', event => {
    writeAtom(commitsShownRange, event.rangeInDays);
  }),
  import.meta.hot,
);

/**
 * Latest head commit from original data from the server, without any previews.
 * Prefer using `dagWithPreviews.resolve('.')`, since it includes optimistic state
 * and previews.
 */
export const latestHeadCommit = atom(get => {
  const commits = get(latestCommits);
  return commits.find(commit => commit.isHead);
});

/**
 * No longer in the "loading" state:
 * - Either the list of commits has successfully loaded
 * - or there was an error during the fetch
 */
export const haveCommitsLoadedYet = atom(get => {
  const data = get(latestCommitsData);
  return data.commits.length > 0 || data.error != null;
});

export const operationBeingPreviewed = atomResetOnCwdChange<Operation | undefined>(undefined);

export const haveRemotePath = atom(get => {
  const info = get(repositoryInfo);
  // codeReviewSystem.type is 'unknown' or other values if paths.default is present.
  return info?.type === 'success' && info.codeReviewSystem.type !== 'none';
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
 * The process has exited but exit code is unknown. Usually exit code is one byte.
 * '-1024' is unlikely to conflict with a valid exit code.
 */
export const EXIT_CODE_FORGET = -1024;

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

/**
 * Ask the server if the current operation is still running.
 * The server might send back a "forgot" progress and we can mark
 * the operation as exited. This is useful when the operation exited
 * during disconnection.
 */
export function maybeRemoveForgottenOperation() {
  const list = readAtom(operationList);
  const operationId = list.currentOperation?.operation.id;
  if (operationId != null) {
    serverAPI.postMessage({
      type: 'requestMissedOperationProgress',
      operationId,
    });
  }
}

export const operationList = atomResetOnCwdChange<OperationList>(defaultOperationList());
registerCleanup(
  operationList,
  serverAPI.onSetup(() => maybeRemoveForgottenOperation()),
  import.meta.hot,
);
registerDisposable(
  operationList,
  serverAPI.onMessageOfType('operationProgress', progress => {
    switch (progress.kind) {
      case 'spawn':
        writeAtom(operationList, list => {
          const operation = operationsById.get(progress.id);
          if (operation == null) {
            return list;
          }

          return startNewOperation(operation, list);
        });
        break;
      case 'stdout':
      case 'stderr':
        writeAtom(operationList, current => {
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
        writeAtom(operationList, current => {
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
        writeAtom(operationList, current => {
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
      case 'forgot':
        writeAtom(operationList, current => {
          const currentOperation = current.currentOperation;
          if (currentOperation == null || currentOperation.exitCode != null) {
            return current;
          }

          const {exitCode, timestamp} =
            progress.kind === 'exit'
              ? progress
              : {exitCode: EXIT_CODE_FORGET, timestamp: Date.now()};
          const complete = operationCompletionCallbacks.get(currentOperation.operation.id);
          complete?.(
            exitCode === 0 ? undefined : new Error(`Process exited with code ${exitCode}`),
          );
          operationCompletionCallbacks.delete(currentOperation.operation.id);

          return {
            ...current,
            currentOperation: {
              ...currentOperation,
              exitCode,
              endTime: new Date(timestamp),
              inlineProgress: undefined, // inline progress never lasts after exiting
            },
          };
        });
        break;
    }
  }),
  import.meta.hot,
);

export const inlineProgressByHash = atomFamilyWeak((hash: Hash) =>
  atom(get => {
    const info = get(operationList);
    const inlineProgress = info.currentOperation?.inlineProgress;
    if (inlineProgress == null) {
      return undefined;
    }
    const shortHash = short(hash); // progress messages come indexed by short hash
    return inlineProgress.get(shortHash);
  }),
);

/** We don't send entire operations to the server, since not all fields are serializable.
 * Thus, when the server tells us about the queue of operations, we need to know which operation it's talking about.
 * Store recently run operations by id. Add to this map whenever a new operation is run. Remove when an operation process exits (successfully or unsuccessfully)
 */
const operationsById = new Map<string, Operation>();
/** Store callbacks to run when an operation completes. This is stored outside of the operation since Operations are typically Immutable. */
const operationCompletionCallbacks = new Map<string, (error?: Error) => void>();

export const queuedOperations = atomResetOnCwdChange<Array<Operation>>([]);
registerDisposable(
  queuedOperations,
  serverAPI.onMessageOfType('operationProgress', progress => {
    switch (progress.kind) {
      case 'queue':
      case 'spawn': // spawning doubles as our notification to dequeue the next operation, and includes the new queue state.
        // Update with the latest queue state. We expect this to be sent whenever we try to run a command but it gets queued.
        writeAtom(queuedOperations, () => {
          return progress.queue
            .map(opId => operationsById.get(opId))
            .filter((op): op is Operation => op != null);
        });
        break;
      case 'error':
        writeAtom(queuedOperations, () => []); // empty queue when a command hits an error
        break;
      case 'exit':
        writeAtom(queuedOperations, current => {
          operationsById.delete(progress.id); // we don't need to care about this operation anymore
          if (progress.exitCode != null && progress.exitCode !== 0) {
            // if any process in the queue exits with an error, the entire queue is cleared.
            return [];
          }
          return current;
        });
        break;
    }
  }),
  import.meta.hot,
);

function runOperationImpl(operation: Operation): Promise<undefined | Error> {
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
  const defered = defer<undefined | Error>();
  operationCompletionCallbacks.set(operation.id, (err?: Error) => {
    defered.resolve(err);
  });

  operationsById.set(operation.id, operation);
  const ongoing = readAtom(operationList);

  if (ongoing?.currentOperation != null && ongoing.currentOperation.exitCode == null) {
    const queue = readAtom(queuedOperations);
    // Add to the queue optimistically. The server will tell us the real state of the queue when it gets our run request.
    writeAtom(queuedOperations, [...(queue || []), operation]);
  } else {
    // start a new operation. We need to manage the previous operations
    writeAtom(operationList, list => startNewOperation(operation, list));
  }

  // Check periodically with the server that the process is still running.
  // This is a fallback in case the server cannot send us "exit" messages.
  // This timer will auto disable when currentOperation becomes null.
  currentOperationHeartbeatTimer.enabled = true;

  return defered.promise;
}

const currentOperationHeartbeatTimer = new Timer(() => {
  const currentOp = readAtom(operationList).currentOperation;
  if (currentOp == null || currentOp.endTime != null) {
    // Stop the timer.
    return false;
  }
  maybeRemoveForgottenOperation();
}, 5000);

/**
 * Returns callback to run an operation.
 * Will be queued by the server if other operations are already running.
 * This returns a promise that resolves when this operation has exited
 * (though its optimistic state may not have finished resolving yet).
 * Note: Most callsites won't await this promise, and just use queueing. If you do, you should probably use `throwOnError = true` to detect errors.
 * TODO: should we refactor this into a separate function if you want to await the result, which always throws?
 * Note: There's no need to wait for this promise to resolve before starting another operation,
 * successive operations will queue up with a nicer UX than if you awaited each one.
 */
export function useRunOperation() {
  return useCallback(async (operation: Operation, throwOnError?: boolean): Promise<void> => {
    const result = await runOperationImpl(operation);
    if (result != null && throwOnError) {
      throw result;
    }
  }, []);
}

/**
 * Returns callback to abort the running operation.
 */
export function useAbortRunningOperation() {
  return useCallback((operationId: string) => {
    serverAPI.postMessage({
      type: 'abortRunningOperation',
      operationId,
    });
    const ongoing = readAtom(operationList);
    if (ongoing?.currentOperation?.operation?.id === operationId) {
      // Mark 'aborting' as true.
      writeAtom(operationList, list => {
        const currentOperation = list.currentOperation;
        if (currentOperation != null) {
          return {...list, currentOperation: {aborting: true, ...currentOperation}};
        }
        return list;
      });
    }
  }, []);
}

/**
 * Returns callback to run the operation currently being previewed, or cancel the preview.
 * Set operationBeingPreviewed to start a preview.
 */
export function useRunPreviewedOperation() {
  return useCallback((isCancel: boolean, operation?: Operation) => {
    if (isCancel) {
      writeAtom(operationBeingPreviewed, undefined);
      return;
    }

    const operationToRun = operation ?? readAtom(operationBeingPreviewed);
    writeAtom(operationBeingPreviewed, undefined);
    if (operationToRun) {
      runOperationImpl(operationToRun);
    }
  }, []);
}

/**
 * It's possible for optimistic state to be incorrect, e.g. if some assumption about a command is incorrect in an edge case
 * but the command doesn't exit non-zero. This provides a backdoor to clear out all ongoing optimistic state from *previous* commands.
 * Queued commands and the currently running command will not be affected.
 */
export function useClearAllOptimisticState() {
  return useCallback(() => {
    writeAtom(operationList, list => {
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
  }, []);
}

registerDisposable(
  serverAPI,
  serverAPI.onMessageOfType('getUiState', () => {
    const state = readInterestingAtoms();
    window.clientToServerAPI?.postMessage({
      type: 'gotUiState',
      state: JSON.stringify(serializeAtomsState(state), undefined, 2),
    });
  }),
  import.meta.hot,
);
