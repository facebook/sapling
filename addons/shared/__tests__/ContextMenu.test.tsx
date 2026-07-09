/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @jest-environment jsdom
 */

import '@testing-library/jest-dom';
import {act, fireEvent, render, screen} from '@testing-library/react';
import {Provider} from 'jotai';
import {computeContextMenuStyle, ContextMenus, useContextMenu} from '../ContextMenu';

const onClick1 = jest.fn();
const onClick2 = jest.fn();

function TestComponent() {
  const menu = useContextMenu(() => [
    {label: 'Context item 1', onClick: onClick1},
    {label: 'Context item 2', onClick: onClick2},
  ]);
  return (
    <div data-testid="test-component" onContextMenu={menu}>
      Hello click me
    </div>
  );
}

function TestApp() {
  return (
    <Provider>
      <div>
        <TestComponent />
        <ContextMenus />
      </div>
    </Provider>
  );
}

function rightClick(el: HTMLElement) {
  fireEvent.contextMenu(el);
}

describe('Context Menu', () => {
  it('shows context menu items on right click', () => {
    render(<TestApp />);

    act(() => {
      const component = screen.getByTestId('test-component');
      rightClick(component);
    });

    expect(screen.getByText('Context item 1')).toBeInTheDocument();
    expect(screen.getByText('Context item 2')).toBeInTheDocument();
  });

  it('runs callbacks on clicking an item', () => {
    render(<TestApp />);

    act(() => {
      const component = screen.getByTestId('test-component');
      rightClick(component);
    });

    act(() => {
      fireEvent.click(screen.getByText('Context item 1'));
    });
    expect(onClick1).toHaveBeenCalled();
    expect(onClick2).not.toHaveBeenCalled();
  });

  it('dismisses on escape key', () => {
    render(<TestApp />);

    act(() => {
      const component = screen.getByTestId('test-component');
      rightClick(component);
    });

    act(() => {
      fireEvent.keyUp(window, {key: 'Escape'});
    });

    expect(screen.queryByText('Context item 1')).not.toBeInTheDocument();
    expect(screen.queryByText('Context item 2')).not.toBeInTheDocument();
  });

  it('dismisses on click outside', () => {
    render(<TestApp />);

    act(() => {
      const component = screen.getByTestId('test-component');
      rightClick(component);
    });

    act(() => {
      fireEvent.click(screen.getByTestId('test-component'));
    });

    expect(screen.queryByText('Context item 1')).not.toBeInTheDocument();
    expect(screen.queryByText('Context item 2')).not.toBeInTheDocument();
  });
});

describe('computeContextMenuStyle', () => {
  const wideWindow = {innerWidth: 1400, innerHeight: 900};
  const narrowWindow = {innerWidth: 460, innerHeight: 900};

  it('keeps the 500px width cap when the viewport has room', () => {
    const {position, leftOrRight} = computeContextMenuStyle({x: 800, y: 430}, wideWindow, 1);
    expect(leftOrRight).toBe('right');
    expect(position.maxWidth).toBe(500);
  });

  it('clamps a right-anchored menu so it never crosses the left edge of a narrow pane', () => {
    // Regression test: the "Rebase onto…" menu used to grow off the left edge in
    // a narrow pane because only the height was clamped to the viewport.
    const {position, leftOrRight} = computeContextMenuStyle({x: 355, y: 430}, narrowWindow, 1);
    expect(leftOrRight).toBe('right');
    const right = position.right as number;
    const maxWidth = position.maxWidth as number;
    expect(maxWidth).toBeLessThan(500);
    // The menu's left edge is innerWidth - right - maxWidth; it must stay >= 0.
    expect(right + maxWidth).toBeLessThanOrEqual(narrowWindow.innerWidth);
  });

  it('clamps a left-anchored menu so it never crosses the right edge of a narrow pane', () => {
    const {position, leftOrRight} = computeContextMenuStyle({x: 100, y: 430}, narrowWindow, 1);
    expect(leftOrRight).toBe('left');
    const left = position.left as number;
    const maxWidth = position.maxWidth as number;
    expect(left + maxWidth).toBeLessThanOrEqual(narrowWindow.innerWidth);
  });

  it('still clamps the height to the viewport', () => {
    const {position, topOrBottom} = computeContextMenuStyle({x: 355, y: 430}, narrowWindow, 1);
    expect(topOrBottom).toBe('top');
    expect(position.maxHeight).toBe(narrowWindow.innerHeight - (position.top as number));
  });

  it('accounts for the zoom level when clamping to the viewport', () => {
    // Click coordinates are already divided by zoom (see useContextMenu) and the
    // window size is divided here too, so the clamp is computed in the same space.
    const zoom = 2;
    const {position} = computeContextMenuStyle(
      {x: 355, y: 430},
      {innerWidth: 920, innerHeight: 1800},
      zoom,
    );
    const anchor = (position.right as number | undefined) ?? (position.left as number);
    expect((position.maxWidth as number) + anchor).toBeLessThanOrEqual(920 / zoom);
  });
});
