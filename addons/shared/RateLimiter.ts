/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Deferred} from './utils';

import {defer} from './utils';

type Id = number;

type QueuedTask = {
  id: Id;
  /** Resolved by the limiter to hand this task its turn. */
  allowedToRun: Deferred<void>;
};

/**
 * Rate limits requests to run an arbitrary task.
 * Up to `maxSimultaneousRunning` tasks can run at once,
 * further requests will be queued and run when a running task finishes.
 *
 * Usage:
 * ```
 * const rateLimiter = new RateLimiter(5);
 * const result = await rateLimiter.enqueueRun(() => {
 *   // ...do arbitrary async work...
 * });
 * ```
 */
export class RateLimiter {
  private queued: Array<QueuedTask> = [];
  private running: Array<Id> = [];

  constructor(
    private maxSimultaneousRunning: number,
    private log?: (s: string) => unknown,
  ) {}

  private nextId = 1;
  private generateId(): Id {
    return this.nextId++;
  }

  async enqueueRun<T>(runner: () => Promise<T>): Promise<T> {
    const id = this.generateId();
    // Created before the task is queued so that `run` can always hand out the turn, whether or not
    // this function has reached the `await` below by the time the turn is granted.
    const task: QueuedTask = {id, allowedToRun: defer<void>()};

    this.queued.push(task);
    this.tryDequeueNext();

    if (!this.running.includes(id)) {
      this.log?.(`${this.running.length} tasks are already running, enqueuing ID:${id}`);
      await task.allowedToRun.promise;
      this.log?.(`now allowing ID:${id} to run`);
    }

    try {
      return await runner();
    } finally {
      this.notifyFinished(id);
    }
  }

  private notifyFinished(id: Id): void {
    this.running = this.running.filter(running => running !== id);
    this.tryDequeueNext();
  }

  private tryDequeueNext() {
    if (this.running.length < this.maxSimultaneousRunning) {
      const toRun = this.queued.shift();
      if (toRun != null) {
        this.run(toRun);
      }
    }
  }

  private run(task: QueuedTask) {
    this.running.push(task.id);
    task.allowedToRun.resolve(undefined);
  }
}
