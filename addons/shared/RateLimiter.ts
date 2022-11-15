/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {TypedEventEmitter} from './TypedEventEmitter';

type Id = number;

/**
 * Rate limits requests to run an arbitrary task.
 * Up to `maxSimultaneousRunning` tasks can run at once,
 * futher requests will be queued and run when a running task finishes.
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
  private queued: Array<Id> = [];
  private running: Array<Id> = [];
  private runs = new TypedEventEmitter<'run', Id>();

  constructor(private maxSimultaneousRunning: number, private log?: (s: string) => unknown) {}

  private nextId = 1;
  private generateId(): Id {
    return this.nextId++;
  }

  async enqueueRun<T>(runner: () => Promise<T>): Promise<T> {
    const id = this.generateId();

    this.queued.push(id);
    this.tryDequeueNext();

    if (!this.running.includes(id)) {
      this.log?.(`${this.running.length} tasks are already running, enqueuing ID:${id}`);
      await new Promise(res => {
        this.runs.on('run', ran => {
          if (ran === id) {
            this.log?.(`now allowing ID:${id} to run`);
            res(undefined);
          }
        });
      });
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

  private run(id: Id) {
    this.running.push(id);
    this.runs.emit('run', id);
  }
}
