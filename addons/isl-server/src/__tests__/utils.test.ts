/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {serializeAsyncCall, sleep} from '../utils';

function flushPromises() {
  return new Promise(res => jest.requireActual('timers').setTimeout(res, 0));
}

describe('serializeAsyncCall', () => {
  it('only runs one at a time', async () => {
    jest.useFakeTimers();
    const runTime = async (t: number) => {
      jest.advanceTimersByTime(t);
      await flushPromises();
    };
    let nextId = 0;
    const started: Array<number> = [];
    const finished: Array<number> = [];
    const testFn = serializeAsyncCall(async () => {
      const id = nextId++;
      started.push(id);
      await sleep(40);
      finished.push(id);
    });

    testFn(); // This one is run immediately
    expect(started).toEqual([0]);
    expect(finished).toEqual([]); // not finished running
    await runTime(10);
    testFn(); // this one queus up while the first is still running
    expect(started).toEqual([0]); // 1 not running yet
    expect(finished).toEqual([]);
    await runTime(10);
    testFn(); // we already have an invocation queued,
    expect(started).toEqual([0]);
    expect(finished).toEqual([]);

    await runTime(60);
    expect(started).toEqual([0, 1]);
    expect(finished).toEqual([0]);

    await runTime(60);
    expect(started).toEqual([0, 1]);
    expect(finished).toEqual([0, 1]);

    // nothing more to run
    await runTime(100);
    expect(started).toEqual([0, 1]);
    expect(finished).toEqual([0, 1]);
  });

  it('returns the same result beyond being queued once', async () => {
    jest.useFakeTimers();
    let callNumber = 1;
    const testFn = serializeAsyncCall(() => {
      return Promise.resolve(callNumber++);
    });

    const promise1 = testFn();
    const promise2 = testFn();
    const promise3 = testFn();

    const values = await Promise.all([promise1, promise2, promise3]);
    expect(values).toEqual([1, 2, 2]);
  });
});
