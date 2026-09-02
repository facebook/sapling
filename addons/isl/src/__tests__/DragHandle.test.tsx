/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type React from 'react';

import {dragHandleProps} from '../DragHandle';

/** jsdom lacks a PointerEvent constructor; emulate one with a pointerId. */
function pointerEvent(
  type: string,
  init: {clientX: number; clientY: number; pointerId?: number},
): MouseEvent {
  const event = new MouseEvent(type, {clientX: init.clientX, clientY: init.clientY});
  Object.defineProperty(event, 'pointerId', {value: init.pointerId ?? 3});
  return event;
}

describe('dragHandleProps', () => {
  const originalRequestAnimationFrame = window.requestAnimationFrame;
  const originalCancelAnimationFrame = window.cancelAnimationFrame;
  const originalSetPointerCapture = document.body.setPointerCapture;
  const originalReleasePointerCapture = document.body.releasePointerCapture;

  afterEach(() => {
    document.body.replaceChildren();
    Object.defineProperties(window, {
      requestAnimationFrame: {configurable: true, value: originalRequestAnimationFrame},
      cancelAnimationFrame: {configurable: true, value: originalCancelAnimationFrame},
    });
    Object.defineProperties(document.body, {
      setPointerCapture: {configurable: true, value: originalSetPointerCapture},
      releasePointerCapture: {configurable: true, value: originalReleasePointerCapture},
    });
  });

  function mockAnimationFrames() {
    let scheduledFrame: FrameRequestCallback | undefined;
    const requestAnimationFrame = jest.fn((callback: FrameRequestCallback) => {
      scheduledFrame = callback;
      return 1;
    });
    const cancelAnimationFrame = jest.fn();
    Object.defineProperties(window, {
      requestAnimationFrame: {configurable: true, value: requestAnimationFrame},
      cancelAnimationFrame: {configurable: true, value: cancelAnimationFrame},
    });
    return {
      requestAnimationFrame,
      cancelAnimationFrame,
      runScheduledFrame: () => scheduledFrame?.(0),
    };
  }

  function mockPointerCapture() {
    const setPointerCapture = jest.fn();
    const releasePointerCapture = jest.fn();
    Object.defineProperties(document.body, {
      setPointerCapture: {configurable: true, value: setPointerCapture},
      releasePointerCapture: {configurable: true, value: releasePointerCapture},
    });
    return {setPointerCapture, releasePointerCapture};
  }

  function startDrag(
    props: ReturnType<typeof dragHandleProps>,
    init: {clientX: number; clientY: number; pointerId?: number},
  ) {
    const handle = document.createElement('span');
    document.body.appendChild(handle);
    props.onPointerDown?.({
      isPrimary: true,
      target: handle,
      clientX: init.clientX,
      clientY: init.clientY,
      pointerId: init.pointerId ?? 3,
    } as unknown as React.PointerEvent);
  }

  it('coalesces pointer moves and flushes the final position on pointer up', () => {
    const {requestAnimationFrame, cancelAnimationFrame, runScheduledFrame} = mockAnimationFrames();
    mockPointerCapture();

    const onDrag = jest.fn();
    const props = dragHandleProps(onDrag);
    startDrag(props, {clientX: 1, clientY: 2});

    document.body.dispatchEvent(pointerEvent('pointermove', {clientX: 4, clientY: 5}));
    document.body.dispatchEvent(pointerEvent('pointermove', {clientX: 6, clientY: 7}));

    expect(requestAnimationFrame).toHaveBeenCalledTimes(1);
    expect(onDrag).toHaveBeenCalledTimes(1);

    document.body.dispatchEvent(pointerEvent('pointerup', {clientX: 8, clientY: 9}));

    expect(cancelAnimationFrame).toHaveBeenCalledWith(1);
    expect(onDrag.mock.calls).toEqual([
      [1, 2, true],
      [8, 9, true],
      [8, 9, false],
    ]);

    runScheduledFrame();
    expect(onDrag).toHaveBeenCalledTimes(3);
  });

  it('does not repeat the dragging callback when pointer up has no pending move', () => {
    mockPointerCapture();

    const onDrag = jest.fn();
    const props = dragHandleProps(onDrag);
    startDrag(props, {clientX: 1, clientY: 2});

    document.body.dispatchEvent(pointerEvent('pointerup', {clientX: 1, clientY: 2}));

    expect(onDrag.mock.calls).toEqual([
      [1, 2, true],
      [1, 2, false],
    ]);
  });

  it('does not repeat a move that was already delivered by the animation frame', () => {
    const {runScheduledFrame} = mockAnimationFrames();
    mockPointerCapture();

    const onDrag = jest.fn();
    const props = dragHandleProps(onDrag);
    startDrag(props, {clientX: 1, clientY: 2});
    document.body.dispatchEvent(pointerEvent('pointermove', {clientX: 4, clientY: 5}));

    runScheduledFrame();
    document.body.dispatchEvent(pointerEvent('pointerup', {clientX: 8, clientY: 9}));

    expect(onDrag.mock.calls).toEqual([
      [1, 2, true],
      [4, 5, true],
      [8, 9, false],
    ]);
  });

  it('ends the drag on pointercancel without applying the pending move', () => {
    const {cancelAnimationFrame} = mockAnimationFrames();
    const {releasePointerCapture} = mockPointerCapture();

    const onDrag = jest.fn();
    const props = dragHandleProps(onDrag);
    startDrag(props, {clientX: 1, clientY: 2});
    document.body.dispatchEvent(pointerEvent('pointermove', {clientX: 4, clientY: 5}));

    // Engines may report degenerate (0,0) coordinates on pointercancel.
    document.body.dispatchEvent(pointerEvent('pointercancel', {clientX: 0, clientY: 0}));

    expect(onDrag.mock.calls).toEqual([
      [1, 2, true],
      [0, 0, false],
    ]);
    expect(cancelAnimationFrame).toHaveBeenCalledWith(1);
    expect(releasePointerCapture).toHaveBeenCalledWith(3);
    expect(document.body.style.cursor).toBe('');

    document.body.dispatchEvent(pointerEvent('pointermove', {clientX: 6, clientY: 7}));
    expect(onDrag).toHaveBeenCalledTimes(2);
  });

  it('ignores moves and ups from other pointers', () => {
    const {requestAnimationFrame, runScheduledFrame} = mockAnimationFrames();
    mockPointerCapture();

    const onDrag = jest.fn();
    const props = dragHandleProps(onDrag);
    startDrag(props, {clientX: 1, clientY: 2, pointerId: 3});

    document.body.dispatchEvent(
      pointerEvent('pointermove', {clientX: 40, clientY: 50, pointerId: 7}),
    );
    document.body.dispatchEvent(
      pointerEvent('pointerup', {clientX: 40, clientY: 50, pointerId: 7}),
    );
    expect(requestAnimationFrame).not.toHaveBeenCalled();
    expect(onDrag).toHaveBeenCalledTimes(1);

    document.body.dispatchEvent(pointerEvent('pointermove', {clientX: 4, clientY: 5}));
    runScheduledFrame();
    document.body.dispatchEvent(pointerEvent('pointerup', {clientX: 8, clientY: 9}));

    expect(onDrag.mock.calls).toEqual([
      [1, 2, true],
      [4, 5, true],
      [8, 9, false],
    ]);
  });

  it('does not start a second session while one is active', () => {
    const {setPointerCapture} = mockPointerCapture();

    const onDrag = jest.fn();
    const props = dragHandleProps(onDrag);
    startDrag(props, {clientX: 1, clientY: 2, pointerId: 3});
    startDrag(props, {clientX: 10, clientY: 20, pointerId: 4});

    expect(setPointerCapture).toHaveBeenCalledTimes(1);
    expect(onDrag).toHaveBeenCalledTimes(1);

    document.body.dispatchEvent(pointerEvent('pointerup', {clientX: 1, clientY: 2}));

    startDrag(props, {clientX: 10, clientY: 20, pointerId: 4});
    expect(setPointerCapture).toHaveBeenCalledTimes(2);
    expect(setPointerCapture).toHaveBeenLastCalledWith(4);
  });
});
