/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Logger} from './logger';
import type {
  OperationCommandProgressReporter,
  OperationProgress,
  RunnableOperation,
} from 'isl/src/types';

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
    ) => Promise<void>,
  ) {}

  private queuedOperations: Array<RunnableOperation> = [];
  private runningOperation: RunnableOperation | undefined = undefined;

  async runOrQueueOperation(
    operation: RunnableOperation,
    onProgress: (progress: OperationProgress) => void,
    cwd: string,
  ): Promise<void> {
    if (this.runningOperation != null) {
      this.queuedOperations.push(operation);
      onProgress({id: operation.id, kind: 'queue', queue: this.queuedOperations.map(o => o.id)});
      return;
    }
    this.runningOperation = operation;

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
        case 'stderr':
          onProgress({id: operation.id, kind: 'stderr', message: args[1]});
          break;
        case 'exit':
          onProgress({id: operation.id, kind: 'exit', exitCode: args[1]});
          break;
      }
    };

    try {
      await this.runCallback(operation, cwd, handleCommandProgress);
      this.runningOperation = undefined;

      // now that we successfully ran this operation, dequeue the next
      if (this.queuedOperations.length > 0) {
        const op = this.queuedOperations.shift();
        if (op != null) {
          // don't await this, the caller should resolve when the original operation finishes.
          this.runOrQueueOperation(
            op,
            // TODO: we're using the onProgress from the LAST `runOperation`... should we be keeping the newer onProgress in the queued operation?
            onProgress,
            cwd,
          );
        }
      }
    } catch (err) {
      const errString = (err as Error).toString();
      this.logger.log('error running operation: ', operation.args[0], errString);
      onProgress({id: operation.id, kind: 'error', error: errString});
      // clear queue to run when we hit an error
      this.queuedOperations = [];
      this.runningOperation = undefined;
    }
  }
}
