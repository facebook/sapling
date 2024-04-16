/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {stagedThrottler} from '../StagedThrottler';

describe('stagedThrottler', () => {
  /**
   * Given an input string, call the throttled function up to once per 1ms,
   * according to the pattern given.
   * e.g. **...** means to:
   *   emit 0, wait 1ms, emit 1, wait 1 ms,
   *   not emit for 3ms
   *   emit 5, wait 1ms, emit 6, wait 1ms.
   */
  const simulate = (input: (i: number) => unknown, values: string) => {
    for (const [i, c] of Array.from(values).entries()) {
      if (c === '*') {
        input(i);
      }
      jest.advanceTimersByTime(1);
    }
  };

  /**
   * Convert the calls to the mocked throttled function into a string of 1s and 0s.
   * 0 means it was not called during that ms, 1 means it was called.
   * This can be compared to the input to `simulate`.
   */
  const expectCall = (throttled: jest.Mock, values: string) => {
    const calls = Array.from(values).map(_c => false);
    for (const call of throttled.mock.calls) {
      calls[call[0]] = true;
    }
    const found = calls.map(v => (v ? '1' : '0')).join('');

    expect(found).toEqual(values);
  };

  beforeEach(() => {
    jest.useFakeTimers().setSystemTime(new Date('2024-01-01'));
  });
  afterEach(() => {
    jest.useRealTimers();
  });

  it('debounce increases through stages', () => {
    const onValue = jest.fn();
    const throttled = stagedThrottler(
      [
        {throttleMs: 0, numToNextStage: 3, resetAfterMs: 10},
        {throttleMs: 5, numToNextStage: 10, resetAfterMs: 10},
        {throttleMs: 10, resetAfterMs: 10},
      ],
      onValue,
    );

    // emit every 1ms
    simulate(throttled, '**************************');
    expectCall(onValue, '11100001000010000000001000');
    // no throttling     ***
    // enter stage 1     --^
    // 5ms throttle             *    *
    // enter stage 2        ----------^
    // 10ms throttle                           *
  });

  it('resets back to stage 0 after rest interval passes', () => {
    const onValue = jest.fn();
    const throttled = stagedThrottler(
      [
        {throttleMs: 0, numToNextStage: 3, resetAfterMs: 10},
        {throttleMs: 5, numToNextStage: 10, resetAfterMs: 10},
        {throttleMs: 10, resetAfterMs: 10},
      ],
      onValue,
    );

    // emit for a while, then stop for a while
    simulate(throttled, '******...............*******');
    expectCall(onValue, '1110000000000000000001111000');
    // no throttling     ***
    // enter stage 1     --^
    // reset to stage 0        ------------^
    // no throttling                           ***
    // enter stage 1                           --^
  });

  it('calls onEnter callbacks', () => {
    const onValue = jest.fn();
    const enterStage0 = jest.fn();
    const enterStage1 = jest.fn();
    const enterStage2 = jest.fn();
    const enterStage3 = jest.fn();
    const throttled = stagedThrottler(
      [
        {throttleMs: 0, numToNextStage: 3, resetAfterMs: 10, onEnter: enterStage0},
        {throttleMs: 5, numToNextStage: 5, resetAfterMs: 10, onEnter: enterStage1},
        {throttleMs: 10, numToNextStage: 20, resetAfterMs: 10, onEnter: enterStage2},
        {throttleMs: 20, resetAfterMs: 10, onEnter: enterStage3},
      ],
      onValue,
    );

    // emit every 1ms
    simulate(throttled, '**************************');
    // enter stage 1     --^
    // enter stage 2        ---------^

    expect(enterStage0).not.toHaveBeenCalled();
    expect(enterStage1).toHaveBeenCalledTimes(1);
    expect(enterStage2).toHaveBeenCalledTimes(1);
    expect(enterStage3).not.toHaveBeenCalled();
  });

  it('calls onEnter callback when resetting', () => {
    const onValue = jest.fn();
    const enterStage0 = jest.fn();
    const enterStage1 = jest.fn();
    const enterStage2 = jest.fn();
    const throttled = stagedThrottler(
      [
        {throttleMs: 0, numToNextStage: 3, resetAfterMs: 10, onEnter: enterStage0},
        {throttleMs: 5, numToNextStage: 20, resetAfterMs: 10, onEnter: enterStage1},
        {throttleMs: 20, resetAfterMs: 10, onEnter: enterStage2},
      ],
      onValue,
    );

    // emit for a while, then stop for a while
    simulate(throttled, '******...............*******');
    // no throttling     ***
    // enter stage 1     --^
    // reset to stage 0        ------------^
    // no throttling                           ***
    // enter stage 1                           --^

    expect(enterStage0).toHaveBeenCalledTimes(1);
    expect(enterStage1).toHaveBeenCalledTimes(2);
    expect(enterStage2).toHaveBeenCalledTimes(0);
  });
});
