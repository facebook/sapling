/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @jest-environment jsdom
 */

import {useThrottledEffect} from '../hooks';
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

    expect(myFunc).toHaveBeenCalledTimes(1);
    expect(myFunc).toHaveBeenCalledWith(0);

    expect(onRender).toHaveBeenCalledTimes(3);
  });
});
