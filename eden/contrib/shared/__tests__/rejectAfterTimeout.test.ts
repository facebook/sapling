/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import rejectAfterTimeout from '../rejectAfterTimeout';

describe('rejectAfterTimeout', () => {
  beforeEach(() => {
    jest.useFakeTimers();
  });

  test('instant promise should win', async () => {
    const instantPromise = Promise.resolve('winner');
    const result = await rejectAfterTimeout(instantPromise, 5_000, 'too slow?');
    expect(result).toBe('winner');
    jest.advanceTimersByTime(5_000);
  });

  test('fast promise should win', async () => {
    const fastPromise = new Promise(resolve => setTimeout(() => resolve('winner'), 1_000));
    const promise = rejectAfterTimeout(fastPromise, 5_000, 'too slow?');
    jest.advanceTimersByTime(1_000);
    const result = await promise;
    expect(result).toBe('winner');
    jest.advanceTimersByTime(4_000);
  });

  test('slow promise should lose', () => {
    const slowPromise = new Promise(resolve => setTimeout(() => resolve('winner'), 5_000));
    const promise = rejectAfterTimeout(slowPromise, 1_000, 'too slow?');
    jest.advanceTimersByTime(1_000);
    expect(promise).rejects.toBe('too slow?');
    jest.advanceTimersByTime(4_000);
  });
});
