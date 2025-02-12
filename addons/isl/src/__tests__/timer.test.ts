/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {gc} from 'shared/testUtils';
import {Timer} from '../timer';

describe('Timer', () => {
  beforeEach(() => {
    jest.useFakeTimers();
  });
  afterEach(() => {
    jest.useRealTimers();
  });

  it('can enable and disable', () => {
    const callback = jest.fn();
    const timer = new Timer(callback, 100);

    timer.enabled = true;
    jest.advanceTimersByTime(250);
    expect(callback).toHaveBeenCalledTimes(2);

    timer.enabled = false;
    jest.advanceTimersByTime(500);
    expect(callback).toHaveBeenCalledTimes(2);

    timer.enabled = true;
    jest.advanceTimersByTime(200);
    expect(callback).toHaveBeenCalledTimes(4);
  });

  it('error once cancels the timer', () => {
    const callback = jest.fn(() => {
      throw new Error('x');
    });

    // Initially enabled.
    const timer = new Timer(callback, 100, true);
    expect(timer.enabled).toBe(true);

    // Try to call 3 times, but the first time it will throw.
    try {
      jest.advanceTimersByTime(350);
    } catch (_e) {}

    // After throw the timer is disabled.
    expect(timer.enabled).toBe(false);
    expect(callback).toHaveBeenCalledTimes(1);
  });

  it('returning false stops the timer', () => {
    let count = 0;
    const callback = jest.fn(() => {
      count += 1;
      return count < 3;
    });
    const timer = new Timer(callback, 100, true);
    expect(timer.enabled).toBe(true);
    jest.advanceTimersByTime(250);
    expect(timer.enabled).toBe(true);
    jest.advanceTimersByTime(500);
    expect(timer.enabled).toBe(false);
    expect(callback).toHaveBeenCalledTimes(3);
  });

  it('dispose cancels the timer forever', () => {
    const callback = jest.fn();
    const timer = new Timer(callback, 100);

    timer.enabled = true;
    jest.advanceTimersByTime(50);

    timer.dispose();

    // Cannot be re-enabled.
    timer.enabled = true;
    jest.advanceTimersByTime(500);
    expect(timer.enabled).toBe(false);
    expect(callback).toHaveBeenCalledTimes(0);
  });

  it('GC cancels the timer forever', async () => {
    const callback = jest.fn();
    let timer: Timer | null = new Timer(callback, 100, true);
    expect(timer.enabled).toBe(true);

    // GC the timer.
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    timer = null;
    await gc();

    // Callback should not be called after the time is gone.
    jest.advanceTimersByTime(500);
    expect(callback).toHaveBeenCalledTimes(0);
  });
});
