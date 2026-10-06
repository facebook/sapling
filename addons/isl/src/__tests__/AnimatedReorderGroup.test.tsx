/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {render} from '@testing-library/react';
import {AnimatedReorderGroup} from '../AnimatedReorderGroup';

function rect(top: number): DOMRect {
  return {
    bottom: top + 10,
    height: 10,
    left: 0,
    right: 10,
    top,
    width: 10,
    x: 0,
    y: top,
    toJSON: () => ({}),
  };
}

describe('AnimatedReorderGroup', () => {
  const originalRequestAnimationFrame = window.requestAnimationFrame;
  const originalCancelAnimationFrame = window.cancelAnimationFrame;

  afterEach(() => {
    jest.restoreAllMocks();
    Object.defineProperties(window, {
      requestAnimationFrame: {configurable: true, value: originalRequestAnimationFrame},
      cancelAnimationFrame: {configurable: true, value: originalCancelAnimationFrame},
    });
  });

  it('does not animate when the container and children move together', () => {
    const requestAnimationFrame = jest.fn(() => 1);
    Object.defineProperty(window, 'requestAnimationFrame', {
      configurable: true,
      value: requestAnimationFrame,
    });
    let containerTop = 100;
    jest.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
      this: HTMLElement,
    ) {
      const id = this.getAttribute('data-reorder-id');
      return rect(containerTop + (id === 'b' ? 20 : 0));
    });

    const {rerender} = render(
      <AnimatedReorderGroup>
        {[<div key="a" data-reorder-id="a" />, <div key="b" data-reorder-id="b" />]}
      </AnimatedReorderGroup>,
    );
    containerTop = 200;
    rerender(
      <AnimatedReorderGroup>
        {[<div key="a" data-reorder-id="a" />, <div key="b" data-reorder-id="b" />]}
      </AnimatedReorderGroup>,
    );

    expect(requestAnimationFrame).not.toHaveBeenCalled();
  });

  it('cancels a pending animation frame when unmounted', () => {
    const requestAnimationFrame = jest.fn(() => 7);
    const cancelAnimationFrame = jest.fn();
    Object.defineProperties(window, {
      requestAnimationFrame: {configurable: true, value: requestAnimationFrame},
      cancelAnimationFrame: {configurable: true, value: cancelAnimationFrame},
    });
    let reversed = false;
    jest.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
      this: HTMLElement,
    ) {
      const id = this.getAttribute('data-reorder-id');
      const relativeTop = id === 'a' ? (reversed ? 20 : 0) : reversed ? 0 : 20;
      return rect(100 + relativeTop);
    });

    const {rerender, unmount} = render(
      <AnimatedReorderGroup>
        {[<div key="a" data-reorder-id="a" />, <div key="b" data-reorder-id="b" />]}
      </AnimatedReorderGroup>,
    );
    reversed = true;
    rerender(
      <AnimatedReorderGroup>
        {[<div key="b" data-reorder-id="b" />, <div key="a" data-reorder-id="a" />]}
      </AnimatedReorderGroup>,
    );

    expect(requestAnimationFrame).toHaveBeenCalledTimes(1);
    unmount();
    expect(cancelAnimationFrame).toHaveBeenCalledWith(7);
  });

  it('animates only rows that are on screen before or after they move', () => {
    let frame: FrameRequestCallback | undefined;
    Object.defineProperty(window, 'requestAnimationFrame', {
      configurable: true,
      value: jest.fn((callback: FrameRequestCallback) => {
        frame = callback;
        return 1;
      }),
    });
    const animate = jest.fn(() => ({cancel: jest.fn(), onfinish: null}));
    Object.defineProperty(HTMLElement.prototype, 'animate', {configurable: true, value: animate});
    // jsdom's viewport is 768px tall.
    const topsBeforeAndAfter: Record<string, [number, number]> = {
      onScreen: [100, 140],
      offScreen: [2000, 2040],
      scrollsIntoView: [2000, 300],
    };
    let moved = false;
    jest.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
      this: HTMLElement,
    ) {
      const id = this.getAttribute('data-reorder-id');
      return rect(id == null ? 0 : topsBeforeAndAfter[id][moved ? 1 : 0]);
    });
    const rows = (ids: Array<string>) => ids.map(id => <div key={id} data-reorder-id={id} />);

    try {
      const {rerender} = render(
        <AnimatedReorderGroup>
          {rows(['onScreen', 'offScreen', 'scrollsIntoView'])}
        </AnimatedReorderGroup>,
      );
      moved = true;
      rerender(
        <AnimatedReorderGroup>
          {rows(['scrollsIntoView', 'onScreen', 'offScreen'])}
        </AnimatedReorderGroup>,
      );
      frame?.(0);

      const animated = animate.mock.contexts.map(element =>
        (element as HTMLElement).getAttribute('data-reorder-id'),
      );
      expect(animated.sort()).toEqual(['onScreen', 'scrollsIntoView']);
    } finally {
      delete (HTMLElement.prototype as {animate?: unknown}).animate;
    }
  });
});
