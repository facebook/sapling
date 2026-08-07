/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {RateLimiter} from '../RateLimiter';
import {nextTick} from '../testUtils';
import {defer} from '../utils';

/**
 * Number of tasks parked in `RateLimiter`'s queue waiting for a turn. Each one owns a `Deferred`
 * the limiter still has to resolve, so this array is the only place a waiter can accumulate. It is
 * reachable only through private fields, but asserting on it is the only way to prove queued tasks
 * don't leak.
 */
function waitingTaskCount(rateLimiter: RateLimiter): number {
  const {queued} = rateLimiter as unknown as {queued: Array<unknown>};
  return queued.length;
}

describe('RateLimiter', () => {
  it('immediately invokes if less than max simultaneous requests are running', () => {
    const d1 = defer();
    const d2 = defer();
    const d3 = defer();
    const rateLimiter = new RateLimiter(3);
    let ran1 = false;
    let ran2 = false;
    let ran3 = false;
    rateLimiter.enqueueRun(async () => {
      ran1 = true;
      await d1.promise;
    });
    rateLimiter.enqueueRun(async () => {
      ran2 = true;
      await d2.promise;
    });
    rateLimiter.enqueueRun(async () => {
      ran3 = true;
      await d3.promise;
    });
    expect(ran1).toBe(true);
    expect(ran2).toBe(true);
    expect(ran3).toBe(true);
  });

  it('queues requests over max simultaneous until a previous task finishes', async () => {
    const d1 = defer();
    const d2 = defer();
    const rateLimiter = new RateLimiter(2);
    rateLimiter.enqueueRun(() => d1.promise);
    rateLimiter.enqueueRun(() => d2.promise);

    let hasId3Resolved = false;
    rateLimiter
      .enqueueRun(() => Promise.resolve())
      .then(() => {
        hasId3Resolved = true;
      });
    expect(hasId3Resolved).toBe(false);

    d2.resolve(undefined);
    await nextTick();
    expect(hasId3Resolved).toBe(true);
  });

  it('can be used as a lock with concurrency limit 1', async () => {
    const d1 = defer();
    const d2 = defer();
    const d3 = defer();
    const rateLimiter = new RateLimiter(1);
    let ran1 = false;
    let ran2 = false;
    let ran3 = false;
    rateLimiter.enqueueRun(async () => {
      await d1.promise;
      ran1 = true;
    });
    rateLimiter.enqueueRun(async () => {
      await d2.promise;
      ran2 = true;
    });
    rateLimiter.enqueueRun(async () => {
      await d3.promise;
      ran3 = true;
    });

    expect(ran1).toBe(false);
    expect(ran2).toBe(false);
    expect(ran3).toBe(false);

    d1.resolve(undefined);
    await nextTick();

    expect(ran1).toBe(true);
    expect(ran2).toBe(false);
    expect(ran3).toBe(false);

    d2.resolve(undefined);
    await nextTick();

    expect(ran1).toBe(true);
    expect(ran2).toBe(true);
    expect(ran3).toBe(false);

    d3.resolve(undefined);
    await nextTick();

    expect(ran1).toBe(true);
    expect(ran2).toBe(true);
    expect(ran3).toBe(true);
  });

  it('Handles async work that rejects', async () => {
    const d1 = defer();
    const d2 = defer();
    const rateLimiter = new RateLimiter(2);
    rateLimiter.enqueueRun(() => d1.promise);
    let sawError = false;
    rateLimiter
      .enqueueRun(async () => {
        await d2.promise;
        throw new Error();
      })
      .catch(() => {
        sawError = true;
      });

    let hasId3Resolved = false;
    rateLimiter
      .enqueueRun(() => Promise.resolve())
      .then(() => {
        hasId3Resolved = true;
      });
    expect(hasId3Resolved).toBe(false);

    d2.resolve(undefined);
    await nextTick();
    expect(hasId3Resolved).toBe(true);
    expect(sawError).toBe(true);
  });

  it('parks one waiter per queued task and releases it once that task may run', async () => {
    const rateLimiter = new RateLimiter(1);
    expect(waitingTaskCount(rateLimiter)).toBe(0);

    const total = 5;
    const deferreds = Array.from({length: total}, () => defer<number>());
    const results = deferreds.map((deferred, i) =>
      rateLimiter.enqueueRun(() => deferred.promise.then(() => i)),
    );

    await nextTick();
    // every task but the running one is waiting for its turn
    expect(waitingTaskCount(rateLimiter)).toBe(total - 1);

    // letting one task through only releases that task
    deferreds[0].resolve(0);
    await nextTick();
    expect(waitingTaskCount(rateLimiter)).toBe(total - 2);

    deferreds.forEach((deferred, i) => deferred.resolve(i));
    expect(await Promise.all(results)).toEqual([0, 1, 2, 3, 4]);
    expect(waitingTaskCount(rateLimiter)).toBe(0);
  });

  it('does not accumulate waiters over the lifetime of the limiter', async () => {
    const rateLimiter = new RateLimiter(1);
    // Guards this design's own accumulator, NOT the per-waiter subscription leak that motivated it
    // — `queued` drained correctly even in the leaking version, so this passes against it. The
    // regression guard for that leak is the `addEventListener` spy in the next test.
    // Runs one task while a second waits, so the number of waiters alive at any moment is 1
    // no matter how many pairs run: the count can only grow if finished tasks stay in the queue.
    const runPair = async (remaining: number): Promise<void> => {
      if (remaining === 0) {
        return;
      }
      const first = defer<void>();
      const second = defer<void>();
      const runs = [
        rateLimiter.enqueueRun(() => first.promise),
        rateLimiter.enqueueRun(() => second.promise),
      ];

      await nextTick();
      expect(waitingTaskCount(rateLimiter)).toBe(1);

      first.resolve(undefined);
      second.resolve(undefined);
      await Promise.all(runs);
      expect(waitingTaskCount(rateLimiter)).toBe(0);

      return runPair(remaining - 1);
    };

    await runPair(20);
  });

  it('adds no EventTarget listener for a burst of waiters past the warning threshold', async () => {
    // This is the regression guard for the per-waiter subscription leak. The EventTarget flavor of
    // `MaxListenersExceededWarning` can only come from `addEventListener`, and `TypedEventEmitter`
    // wraps an `EventTarget`, so never calling it is what makes that warning unreachable rather
    // than merely rare. (Node raises the same warning from `EventEmitter.addListener` too, which
    // this does not cover — parking waiters on a `node:events` emitter would reintroduce it.)
    // Capturing `process.on('warning')` cannot prove this here: jest-environment-node gives each
    // test a copy of `process`, and warnings raised by the real one never reach that copy.
    const addEventListener = jest.spyOn(EventTarget.prototype, 'addEventListener');
    try {
      const maxSimultaneous = 4;
      const total = 20; // leaves 16 tasks waiting at once, well past the threshold of 10
      const rateLimiter = new RateLimiter(maxSimultaneous);
      const deferreds = Array.from({length: total}, () => defer<void>());
      const started: Array<number> = [];
      const results = deferreds.map((deferred, i) =>
        rateLimiter.enqueueRun(async () => {
          started.push(i);
          await deferred.promise;
          return i;
        }),
      );

      await nextTick();
      expect(waitingTaskCount(rateLimiter)).toBe(total - maxSimultaneous);
      expect(addEventListener).not.toHaveBeenCalled();

      deferreds.forEach(deferred => deferred.resolve(undefined));
      const inOrder = Array.from({length: total}, (_, i) => i);
      expect(await Promise.all(results)).toEqual(inOrder);
      expect(started).toEqual(inOrder);
      expect(waitingTaskCount(rateLimiter)).toBe(0);
      expect(addEventListener).not.toHaveBeenCalled();
    } finally {
      addEventListener.mockRestore();
    }
  });

  it('still runs every task in order without exceeding the concurrency limit', async () => {
    const rateLimiter = new RateLimiter(2);
    const deferreds = Array.from({length: 6}, () => defer<number>());
    const started: Array<number> = [];
    let running = 0;
    let maxRunning = 0;

    const results = deferreds.map((deferred, i) =>
      rateLimiter.enqueueRun(async () => {
        started.push(i);
        running++;
        maxRunning = Math.max(maxRunning, running);
        const result = await deferred.promise;
        running--;
        return result;
      }),
    );

    await nextTick();
    expect(started).toEqual([0, 1]);

    deferreds.forEach((deferred, i) => deferred.resolve(i * 10));
    expect(await Promise.all(results)).toEqual([0, 10, 20, 30, 40, 50]);
    expect(started).toEqual([0, 1, 2, 3, 4, 5]);
    expect(maxRunning).toBe(2);
  });

  it('still logs when a task is enqueued and when it is allowed to run', async () => {
    const log = jest.fn();
    const rateLimiter = new RateLimiter(1, log);
    const d1 = defer<void>();
    const d2 = defer<void>();

    const first = rateLimiter.enqueueRun(() => d1.promise);
    const second = rateLimiter.enqueueRun(() => d2.promise);
    expect(log).toHaveBeenCalledWith('1 tasks are already running, enqueuing ID:2');
    expect(log).not.toHaveBeenCalledWith('now allowing ID:2 to run');

    d1.resolve(undefined);
    await nextTick();
    expect(log).toHaveBeenCalledWith('now allowing ID:2 to run');

    d2.resolve(undefined);
    await Promise.all([first, second]);
    expect(log).toHaveBeenCalledTimes(2);
  });
});
