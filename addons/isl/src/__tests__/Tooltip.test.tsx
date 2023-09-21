/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import App from '../App';
import {Tooltip, TooltipRootContainer} from '../Tooltip';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  closeCommitInfoSidebar,
} from '../testUtils';
import {fireEvent, render, screen, within} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {act} from 'react-dom/test-utils';

/* eslint-disable @typescript-eslint/no-non-null-assertion */

jest.mock('../MessageBus');

describe('tooltips in ISL', () => {
  let unmount: () => void;
  beforeEach(() => {
    resetTestMessages();
    unmount = render(<App />).unmount;

    act(() => {
      closeCommitInfoSidebar();
      expectMessageSentToServer({
        type: 'subscribe',
        kind: 'smartlogCommits',
        subscriptionID: expect.anything(),
      });
      simulateCommits({
        value: [
          COMMIT('1', 'some public base', '0', {phase: 'public'}),
          COMMIT('a', 'My Commit', '1'),
          COMMIT('b', 'Another Commit', 'a', {isHead: true}),
        ],
      });
    });
  });
  afterEach(() => {
    unmount();
  });

  describe('click to show', () => {
    const clickSettingsGearToMakeTooltip = () => {
      const settingsButtonTooltipCreator =
        screen.getByTestId('settings-gear-button').parentElement!;
      expect(settingsButtonTooltipCreator).toBeInTheDocument();
      act(() => {
        fireEvent.click(settingsButtonTooltipCreator);
      });
    };

    it('shows settings dropdown when clicked', () => {
      clickSettingsGearToMakeTooltip();

      const settingsDropdown = within(screen.getByTestId('tooltip-root-container')).getByTestId(
        'settings-dropdown',
      );
      expect(settingsDropdown).toBeInTheDocument();
    });

    it('clicking inside tooltip does not dismiss it', () => {
      clickSettingsGearToMakeTooltip();

      const settingsDropdown = within(screen.getByTestId('tooltip-root-container')).getByTestId(
        'settings-dropdown',
      );
      const themeDropdown = within(settingsDropdown).getByText('Theme');
      expect(themeDropdown).toBeInTheDocument();
      act(() => {
        fireEvent.click(themeDropdown!);
      });

      const settingsDropdown2 = within(screen.getByTestId('tooltip-root-container')).getByTestId(
        'settings-dropdown',
      );
      expect(settingsDropdown2).toBeInTheDocument();
    });

    it('clicking outside tooltip dismisses it', () => {
      const settingsButton = screen.getByTestId('settings-gear-button');
      act(() => {
        fireEvent.click(settingsButton);
      });

      const settingsDropdown = within(screen.getByTestId('tooltip-root-container')).queryByTestId(
        'settings-dropdown',
      );
      expect(settingsDropdown).toBeInTheDocument();

      act(() => {
        fireEvent.click(screen.getByTestId('commit-a')!);
      });

      const settingsDropdown2 = within(screen.getByTestId('tooltip-root-container')).queryByTestId(
        'settings-dropdown',
      );
      expect(settingsDropdown2).not.toBeInTheDocument();
    });
  });

  describe('hover to show', () => {
    const REFRESH_BUTTON_HOVER_TEXT = 'Re-fetch latest commits and uncommitted changes.';
    it('hovering refresh button shows tooltip', () => {
      const refreshButton = screen.getByTestId('refresh-button').parentElement as HTMLElement;
      userEvent.hover(refreshButton);

      const refreshButtonTooltip = within(screen.getByTestId('tooltip-root-container')).getByText(
        REFRESH_BUTTON_HOVER_TEXT,
      );
      expect(refreshButtonTooltip).toBeInTheDocument();

      userEvent.unhover(refreshButton);

      expect(
        within(screen.getByTestId('tooltip-root-container')).queryByText(REFRESH_BUTTON_HOVER_TEXT),
      ).not.toBeInTheDocument();
    });

    it('escape key dismisses tooltip', () => {
      const refreshButton = screen.getByTestId('refresh-button').parentElement as HTMLElement;
      userEvent.hover(refreshButton);

      const refreshButtonTooltip = within(screen.getByTestId('tooltip-root-container')).getByText(
        REFRESH_BUTTON_HOVER_TEXT,
      );
      expect(refreshButtonTooltip).toBeInTheDocument();

      userEvent.keyboard('{Escape}');

      expect(
        within(screen.getByTestId('tooltip-root-container')).queryByText(REFRESH_BUTTON_HOVER_TEXT),
      ).not.toBeInTheDocument();
    });
  });
});

describe('tooltip', () => {
  function renderCustom(node: ReactNode) {
    render(
      <div className="isl-root">
        <TooltipRootContainer />
        {node}
      </div>,
    );
  }

  describe('onDismiss', () => {
    it('calls onDismiss when hover leaves', () => {
      const onDismiss = jest.fn();
      renderCustom(
        <Tooltip trigger="hover" title="hi" onDismiss={onDismiss}>
          hover me
        </Tooltip>,
      );
      const tooltip = screen.getByText('hover me');
      userEvent.hover(tooltip);
      expect(onDismiss).not.toHaveBeenCalled();
      userEvent.unhover(tooltip);
      expect(onDismiss).toHaveBeenCalledTimes(1);
    });

    it('calls onDismiss when pressing escape', () => {
      const onDismiss = jest.fn();
      renderCustom(
        <Tooltip trigger="hover" title="hi" onDismiss={onDismiss}>
          hover me
        </Tooltip>,
      );
      const tooltip = screen.getByText('hover me');
      userEvent.hover(tooltip);
      expect(onDismiss).not.toHaveBeenCalled();
      userEvent.keyboard('{Escape}');
      expect(onDismiss).toHaveBeenCalledTimes(1);
    });

    it('calls onDismiss when clicking outside', () => {
      const onDismiss = jest.fn();
      renderCustom(
        <div>
          <div>something else</div>
          <Tooltip trigger="click" component={() => <div>hi</div>} onDismiss={onDismiss}>
            click me
          </Tooltip>
        </div>,
      );
      const tooltip = screen.getByText('click me');
      fireEvent.click(tooltip);
      expect(onDismiss).not.toHaveBeenCalled();
      const other = screen.getByText('something else');
      fireEvent.click(other);
      expect(onDismiss).toHaveBeenCalledTimes(1);
    });

    it('title fields on click tooltips does not trigger onDismiss', () => {
      const onDismiss = jest.fn();
      renderCustom(
        <div>
          <div>something else</div>
          <Tooltip
            trigger="click"
            component={() => <div>hi</div>}
            title="hovered"
            onDismiss={onDismiss}>
            click me
          </Tooltip>
        </div>,
      );
      const tooltip = screen.getByText('click me');
      userEvent.hover(tooltip);
      expect(onDismiss).not.toHaveBeenCalled();
      userEvent.unhover(tooltip);
      expect(onDismiss).not.toHaveBeenCalled();
    });

    it('dismiss prop in tooltip components calls onDismiss', () => {
      const onDismiss = jest.fn();
      renderCustom(
        <Tooltip
          trigger="click"
          component={dismiss => (
            <>
              <div>hi</div>
              <button onClick={dismiss}>my button</button>
            </>
          )}
          title="hovered"
          onDismiss={onDismiss}>
          click me
        </Tooltip>,
      );
      const tooltip = screen.getByText('click me');
      fireEvent.click(tooltip);
      expect(onDismiss).not.toHaveBeenCalled();

      // clicking inside tooltip is fine
      const innerText = screen.getByText('hi');
      fireEvent.click(innerText);
      expect(onDismiss).not.toHaveBeenCalled();

      // action that causes dismiss prop causes onDismiss
      const innerDismiss = screen.getByText('my button');
      fireEvent.click(innerDismiss);
      expect(onDismiss).toHaveBeenCalledTimes(1);
    });
  });
});
