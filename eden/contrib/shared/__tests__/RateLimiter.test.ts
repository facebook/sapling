/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {RateLimiter} from '../RateLimiter';
import {nextTick} from '../testUtils';
import {defer} from '../utils';

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
});
