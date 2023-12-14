/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ServerSideTracker} from './analytics/serverSideTracker';
import type {Logger} from './logger';
import type {
  OperationCommandProgressReporter,
  OperationProgress,
  RunnableOperation,
} from 'isl/src/types';

import {newAbortController} from 'shared/compat';

/**
 * Handle running & queueing all Operations so that only one Operation runs at once.
 * Operations may be run by sl in the Repository or other providers like ghstack in the RemoteRepository.
 */
export class OperationQueue {
  constructor(
    private logger: Logger,
    private runCallback: (
      operation: RunnableOperation,
      cwd: string,
      handleProgress: OperationCommandProgressReporter,
      signal: AbortSignal,
    ) => Promise<void>,
  ) {}

  private queuedOperations: Array<RunnableOperation & {tracker: ServerSideTracker}> = [];
  private runningOperation: RunnableOperation | undefined = undefined;
  private runningOperationStartTime: Date | undefined = undefined;
  private abortController: AbortController | undefined = undefined;

  async runOrQueueOperation(
    operation: RunnableOperation,
    onProgress: (progress: OperationProgress) => void,
    tracker: ServerSideTracker,
    cwd: string,
  ): Promise<void> {
    if (this.runningOperation != null) {
      this.queuedOperations.push({...operation, tracker});
      onProgress({id: operation.id, kind: 'queue', queue: this.queuedOperations.map(o => o.id)});
      return;
    }
    this.runningOperation = operation;
    this.runningOperationStartTime = new Date();

    const handleCommandProgress: OperationCommandProgressReporter = (...args) => {
      switch (args[0]) {
        case 'spawn':
          onProgress({
            id: operation.id,
            kind: 'spawn',
            queue: this.queuedOperations.map(op => op.id),
          });
          break;
        case 'stdout':
          onProgress({id: operation.id, kind: 'stdout', message: args[1]});
          break;
        case 'progress':
          onProgress({id: operation.id, kind: 'progress', progress: args[1]});
          break;
        case 'inlineProgress':
          onProgress({id: operation.id, kind: 'inlineProgress', hash: args[1], message: args[2]});
          break;
        case 'stderr':
          onProgress({id: operation.id, kind: 'stderr', message: args[1]});
          break;
        case 'exit':
          onProgress({id: operation.id, kind: 'exit', exitCode: args[1], timestamp: Date.now()});
          break;
      }
    };

    try {
      const controller = newAbortController();
      this.abortController = controller;
      await tracker.operation(
        operation.trackEventName,
        'RunOperationError',
        {extras: {args: operation.args, runner: operation.runner}, operationId: operation.id},
        _p => this.runCallback(operation, cwd, handleCommandProgress, controller.signal),
      );
    } catch (err) {
      const errString = (err as Error).toString();
      this.logger.log('error running operation: ', operation.args[0], errString);
      onProgress({id: operation.id, kind: 'error', error: errString});
      // clear queue to run when we hit an error
      this.queuedOperations = [];
    } finally {
      this.runningOperationStartTime = undefined;
      this.runningOperation = undefined;
    }

    // now that we successfully ran this operation, dequeue the next
    if (this.queuedOperations.length > 0) {
      const op = this.queuedOperations.shift();
      if (op != null) {
        // don't await this, the caller should resolve when the original operation finishes.
        this.runOrQueueOperation(
          op,
          // TODO: we're using the onProgress from the LAST `runOperation`... should we be keeping the newer onProgress in the queued operation?
          onProgress,
          op.tracker,
          cwd,
        );
      }
    }
  }

  /**
   * Get the running operation start time.
   * Returns `undefined` if there is no running operation.
   */
  getRunningOperationStartTime(): Date | undefined {
    if (this.runningOperation == null) {
      return undefined;
    }
    return this.runningOperationStartTime;
  }

  /**
   * Send kill signal to the running operation if the operationId matches.
   * If the process exits, the exit event will be noticed by the queue.
   * This function does not block on waiting for the operation process to exit.
   */
  abortRunningOperation(operationId: string) {
    if (this.runningOperation?.id == operationId) {
      this.abortController?.abort();
    }
  }
}
