/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @jest-environment jsdom
 */

import {useDeepMemo, useThrottledEffect} from '../hooks';
import {fireEvent, render, screen} from '@testing-library/react';
import '@testing-library/jest-dom';
import {useState} from 'react';
import {act} from 'react-dom/test-utils';

describe('useThrottledEffect', () => {
  afterEach(() => {
    jest.useRealTimers();
  });
  it('debounces multiple calls', () => {
    jest.useFakeTimers();
    const myFunc = jest.fn();
    const onRender = jest.fn();
    function MyComponent() {
      const [count, setCount] = useState(0);
      onRender();
      useThrottledEffect(
        () => {
          myFunc(count);
        },
        1000,
        [],
      );
      return <button data-testid="button" onClick={() => setCount(count + 1)} />;
    }

    render(<MyComponent />);
    jest.advanceTimersByTime(100);
    act(() => {
      fireEvent.click(screen.getByTestId('button'));
    });
    jest.advanceTimersByTime(100);
    act(() => {
      fireEvent.click(screen.getByTestId('button'));
    });
    jest.advanceTimersByTime(2000);

    expect(myFunc).toHaveBeenCalledTimes(1);
    expect(myFunc).toHaveBeenCalledWith(0);

    expect(onRender).toHaveBeenCalledTimes(3);
  });

  it('resets via dependencies', () => {
    jest.useFakeTimers();
    const myFunc = jest.fn();
    const onRender = jest.fn();
    function MyComponent() {
      const [count, setCount] = useState(0);
      onRender();
      useThrottledEffect(
        () => {
          myFunc(count);
        },
        1000,
        [count],
      );
      return <button data-testid="button" onClick={() => setCount(count + 1)} />;
    }

    render(<MyComponent />);
    jest.advanceTimersByTime(100);
    act(() => {
      fireEvent.click(screen.getByTestId('button'));
    });
    jest.advanceTimersByTime(100);
    act(() => {
      fireEvent.click(screen.getByTestId('button'));
    });
    jest.advanceTimersByTime(2000);

    expect(myFunc).toHaveBeenCalledTimes(3);
    expect(myFunc).toHaveBeenCalledWith(0);
    expect(myFunc).toHaveBeenCalledWith(1);
    expect(myFunc).toHaveBeenCalledWith(2);

    expect(onRender).toHaveBeenCalledTimes(3);
  });
});

describe('useDeepMemo', () => {
  it('uses deep equality and memoizes', () => {
    jest.useFakeTimers();
    const myExpensiveFunc = jest.fn();

    function MyComponent({dep}: {dep: unknown}) {
      useDeepMemo(myExpensiveFunc, [dep]);
      return <div />;
    }

    const {rerender} = render(<MyComponent dep={{foo: 123}} />);
    expect(myExpensiveFunc).toHaveBeenCalledTimes(1);
    rerender(<MyComponent dep={{foo: 123}} />);
    rerender(<MyComponent dep={{foo: 123}} />);
    rerender(<MyComponent dep={{foo: 123}} />);
    expect(myExpensiveFunc).toHaveBeenCalledTimes(1);
    rerender(<MyComponent dep={{foo: 1234}} />);
    expect(myExpensiveFunc).toHaveBeenCalledTimes(2);
    rerender(<MyComponent dep={{foo: 1234}} />);
    rerender(<MyComponent dep={{foo: 123}} />);
    expect(myExpensiveFunc).toHaveBeenCalledTimes(3);
    rerender(<MyComponent dep={[1, 2, 3]} />);
    expect(myExpensiveFunc).toHaveBeenCalledTimes(4);
    rerender(<MyComponent dep={[1, 2, 3]} />);
    expect(myExpensiveFunc).toHaveBeenCalledTimes(4);
  });
});
