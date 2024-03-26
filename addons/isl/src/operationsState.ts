/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Operation} from './operations/Operation';
import type {Disposable, Hash, ProgressStep, ServerToClientMessage} from './types';
import type {EnsureAssignedTogether} from 'shared/EnsureAssignedTogether';

import serverAPI from './ClientToServerAPI';
import {atomFamilyWeak, readAtom, writeAtom} from './jotaiUtils';
import {atomResetOnCwdChange} from './repositoryData';
import {Timer} from './timer';
import {registerCleanup, registerDisposable, short} from './utils';
import {atom} from 'jotai';
import {useCallback} from 'react';
import {defer} from 'shared/utils';

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

export const operationBeingPreviewed = atomResetOnCwdChange<Operation | undefined>(undefined);

/** We don't send entire operations to the server, since not all fields are serializable.
 * Thus, when the server tells us about the queue of operations, we need to know which operation it's talking about.
 * Store recently run operations by id. Add to this map whenever a new operation is run. Remove when an operation process exits (successfully or unsuccessfully)
 */
const operationsById = new Map<string, Operation>();
/** Store callbacks to run when an operation completes. This is stored outside of the operation since Operations are typically Immutable. */
const operationCompletionCallbacks = new Map<string, (error?: Error) => void>();

/**
 * Subscribe to an operation exiting. Useful for handling cases where an operation fails
 * and it should reset the UI to try again.
 */
export function onOperationExited(
  cb: (
    message: ServerToClientMessage & {type: 'operationProgress'; kind: 'exit'},
    operation: Operation,
  ) => unknown,
): Disposable {
  return serverAPI.onMessageOfType('operationProgress', progress => {
    if (progress.kind === 'exit') {
      const op = operationsById.get(progress.id);
      if (op) {
        cb(progress, op);
      }
    }
  });
}

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
          setTimeout(() => {
            // we don't need to care about this operation anymore after this tick,
            // once all other callsites processing 'operationProgress' messages have run.
            operationsById.delete(progress.id);
          });
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

export function getLastestOperationInfo(operation: Operation): OperationInfo | undefined {
  const list = readAtom(operationList);
  const info =
    list.currentOperation?.operation === operation
      ? list.currentOperation
      : list.operationHistory.find(op => op.operation === operation);

  return info;
}

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
