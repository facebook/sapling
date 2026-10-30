/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ContextMenuItem} from 'shared/ContextMenu';

import {act, render} from '@testing-library/react';

const mockBumpSmartAction = jest.fn();
const mockGetMessagePayloadA = jest.fn();
const mockGetMessagePayloadB = jest.fn();

// Mock `Internal` so that `smartActionsConfig` (read at module load) contains two
// test actions.
jest.mock('../Internal', () => ({
  Internal: {
    smartActions: {
      showSmartActions: true,
      smartActionsConfig: [
        {
          id: 'action-a',
          label: 'Action A',
          trackEventName: 'TestActionA',
          getMessagePayload: (...args: Array<unknown>) => mockGetMessagePayloadA(...args),
        },
        {
          id: 'action-b',
          label: 'Action B',
          trackEventName: 'TestActionB',
          getMessagePayload: (...args: Array<unknown>) => mockGetMessagePayloadB(...args),
        },
      ],
    },
  },
}));

// Pass the input through unchanged so we can rely on the order [Action A, Action B].
// Spy on `bumpSmartAction` so we can verify what got bumped.
jest.mock('../smartActions/smartActionsOrdering', () => ({
  bumpSmartAction: (...args: Array<unknown>) => mockBumpSmartAction(...args),
  useSortedActions: <T,>(items: T) => items,
}));

// Capture the menu items the component would show in its context menu so we can
// invoke their onClick handlers directly without driving the dropdown open.
let mockCapturedItems: Array<ContextMenuItem> = [];
jest.mock('shared/ContextMenu', () => {
  const actual = jest.requireActual('shared/ContextMenu');
  return {
    ...actual,
    useContextMenu: (creator: () => Array<ContextMenuItem>) => {
      mockCapturedItems = creator();
      return jest.fn();
    },
  };
});

import {SmartActionsDropdown} from '../smartActions/SmartActionsDropdown';

/*
 * These tests cover the click-handler change in `SmartActionsDropdown`:
 *   - Both normal and alt clicks should set the selected action and bump usage.
 *   - Neither path should call `runSmartAction` from this handler.
 *   - Only an alt click should defer-open the context tooltip.
 */
describe('SmartActionsDropdown menu item click handler', () => {
  let dispatchEventSpy: jest.SpyInstance;

  beforeEach(() => {
    jest.useFakeTimers();
    mockCapturedItems = [];
    mockBumpSmartAction.mockClear();
    mockGetMessagePayloadA.mockClear();
    mockGetMessagePayloadB.mockClear();
    dispatchEventSpy = jest.spyOn(EventTarget.prototype, 'dispatchEvent');
  });

  afterEach(() => {
    dispatchEventSpy.mockRestore();
    jest.useRealTimers();
  });

  function getMenuItem(label: string): {
    onClick: NonNullable<Extract<ContextMenuItem, {type?: undefined}>['onClick']>;
  } {
    const item = mockCapturedItems.find(i => i.type == null && i.label === label);
    if (!item || item.type != null || !item.onClick) {
      throw new Error(`Could not find clickable menu item with label "${label}"`);
    }
    return {onClick: item.onClick};
  }

  // The component dispatches a 'change' Event on its private `contextTooltipToggle`
  // EventTarget when the tooltip should open. In this isolated render, that is the
  // only source of 'change' events on an EventTarget.
  function changeEventCount(): number {
    return dispatchEventSpy.mock.calls.filter(call => (call[0] as Event)?.type === 'change').length;
  }

  async function renderDropdown(): Promise<void> {
    await act(async () => {
      render(<SmartActionsDropdown />);
    });
    // Allow the loadable feature-flags atom to resolve and trigger a re-render
    // so `useContextMenu` is called with the populated action list.
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
  }

  it('on a normal click: bumps usage but does NOT run the action and does NOT open the tooltip', async () => {
    await renderDropdown();
    expect(mockCapturedItems.length).toBeGreaterThanOrEqual(2);

    const itemA = getMenuItem('Action A');
    const eventsBefore = changeEventCount();

    act(() => {
      itemA.onClick({altKey: false} as MouseEvent);
    });
    act(() => {
      jest.runAllTimers();
    });

    expect(mockBumpSmartAction).toHaveBeenCalledTimes(1);
    expect(mockBumpSmartAction).toHaveBeenCalledWith('action-a');
    // `runSmartAction` would have invoked the action's `getMessagePayload`.
    expect(mockGetMessagePayloadA).not.toHaveBeenCalled();
    expect(mockGetMessagePayloadB).not.toHaveBeenCalled();
    // No context tooltip 'change' event should have been dispatched.
    expect(changeEventCount()).toBe(eventsBefore);
  });

  it('on an alt-click: bumps usage, does NOT run the action, and opens the context tooltip after a deferred tick', async () => {
    await renderDropdown();
    expect(mockCapturedItems.length).toBeGreaterThanOrEqual(2);

    const itemB = getMenuItem('Action B');
    const eventsBefore = changeEventCount();

    act(() => {
      itemB.onClick({altKey: true} as MouseEvent);
    });

    // The tooltip event is deferred via `setTimeout(..., 0)` so it should not
    // have fired yet, but the action should already be bumped.
    expect(changeEventCount()).toBe(eventsBefore);
    expect(mockBumpSmartAction).toHaveBeenCalledTimes(1);
    expect(mockBumpSmartAction).toHaveBeenCalledWith('action-b');

    act(() => {
      jest.runAllTimers();
    });

    // After flushing the deferred timer, exactly one 'change' event must have
    // been dispatched on the contextTooltipToggle EventTarget.
    expect(changeEventCount()).toBe(eventsBefore + 1);
    // `runSmartAction` should still NOT have been invoked.
    expect(mockGetMessagePayloadA).not.toHaveBeenCalled();
    expect(mockGetMessagePayloadB).not.toHaveBeenCalled();
  });
});
