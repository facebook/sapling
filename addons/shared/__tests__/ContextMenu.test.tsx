/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @jest-environment jsdom
 */

import '@testing-library/jest-dom';
import {ContextMenus, useContextMenu} from '../ContextMenu';
import {fireEvent, render, screen} from '@testing-library/react';
import {act} from 'react-dom/test-utils';
import {RecoilRoot} from 'recoil';

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
    <RecoilRoot>
      <div>
        <TestComponent />
        <ContextMenus />
      </div>
    </RecoilRoot>
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
