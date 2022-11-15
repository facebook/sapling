/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {debounce} from '../debounce';

describe('debounce', () => {
  let func1: jest.Mock<[number, string]>;
  const BUFFER = 10;

  beforeEach(() => {
    jest.resetModules();
    func1 = jest.fn();
    jest.useFakeTimers();
  });

  function argsEquivalent(args1: Array<unknown>, args2: Array<unknown>) {
    for (let i = 0; i < Math.max(args1.length, args2.length); i++) {
      if (args1[i] != args2[i]) {
        return false;
      }
    }
    return true;
  }

  function assertCalledWith(...origargs: Array<unknown>) {
    const args = [].slice.call(origargs);
    expect(func1.mock.calls.some(call => argsEquivalent(args, call))).toBeTruthy();
  }

  it('should not call until the wait is over', () => {
    const wait = 200;
    const debounced = debounce(func1, wait);
    debounced(1, 'a');
    expect(func1).not.toBeCalled();

    jest.advanceTimersByTime(wait + BUFFER);
    assertCalledWith(1, 'a');

    // make sure that subsequent function isn't called right away
    debounced(2, 'a');
    expect(func1.mock.calls.length).toBe(1);
    jest.clearAllTimers();
  });

  it('should only call the last function per batch', () => {
    const wait = 200;
    const debounced = debounce(func1, wait);
    debounced(1, 'a');
    expect(func1).not.toBeCalled();
    jest.advanceTimersByTime(100);
    debounced(2, 'a');
    jest.advanceTimersByTime(100);
    debounced(3, 'a');
    jest.advanceTimersByTime(100);
    debounced(4, 'a');
    jest.advanceTimersByTime(100);
    debounced(5, 'a');
    expect(jest.getTimerCount()).toBe(1);
    jest.advanceTimersByTime(wait + BUFFER);
    assertCalledWith(5, 'a');
    debounced(6, 'a');
    debounced(7, 'a');
    jest.advanceTimersByTime(wait + BUFFER);
    assertCalledWith(7, 'a');
    expect(func1.mock.calls.length).toBe(2);
  });

  it('should be reset-able', () => {
    const wait = 300;
    const debounced = debounce(func1, wait);
    debounced(1, 'a');
    debounced.reset();
    expect(jest.getTimerCount()).toBe(0);
    jest.runAllTimers();
    expect(func1).not.toBeCalled();
  });

  it('should correctly show if the timeout is pending', () => {
    const wait = 300;
    const debounced = debounce(func1, wait);
    expect(debounced.isPending()).toBe(false);
    debounced(1, 'a');
    debounced(1, 'a');
    expect(debounced.isPending()).toBe(true);
    jest.runAllTimers();
    expect(func1.mock.calls.length).toBe(1);
    expect(debounced.isPending()).toBe(false);
  });

  describe('leading', () => {
    it('should call the function immediately if able', () => {
      const wait = 300;
      const debounced = debounce(func1, wait, undefined, true);
      debounced(1, 'a');
      expect(func1).toBeCalled();
    });
    it('should gate consecutive calls within the wait time', () => {
      const wait = 300;
      const debounced = debounce(func1, wait, undefined, true);
      debounced(1, 'a');
      debounced(1, 'a');
      debounced(1, 'a');
      debounced(1, 'a');
      debounced(1, 'a');
      expect(func1).toBeCalledTimes(1);
    });
    it('should call the function immediately after the wait time', () => {
      const wait = 300;
      const debounced = debounce(func1, wait, undefined, true);
      debounced(1, 'a');
      debounced(1, 'a');
      debounced(1, 'a');
      debounced(1, 'a');
      debounced(1, 'a');
      expect(func1).toBeCalledTimes(1);
      jest.advanceTimersByTime(wait + BUFFER);
      debounced(1, 'a');
      debounced(1, 'a');
      debounced(1, 'a');
      debounced(1, 'a');
      debounced(1, 'a');
      expect(func1).toBeCalledTimes(2);
    });
    it('should extend the wait time whenever it is called within the wait time', () => {
      const wait = 300;
      const debounced = debounce(func1, wait, undefined, true);
      debounced(1, 'a');
      expect(func1).toBeCalledTimes(1);
      jest.advanceTimersByTime(wait - BUFFER);
      debounced(1, 'a');
      expect(func1).toBeCalledTimes(1);
      jest.advanceTimersByTime(wait - BUFFER);
      debounced(1, 'a');
      expect(func1).toBeCalledTimes(1);
      jest.advanceTimersByTime(wait + BUFFER);
      debounced(1, 'a');
      expect(func1).toBeCalledTimes(2);
    });
  });
});
