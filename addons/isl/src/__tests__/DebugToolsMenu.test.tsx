/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {act, fireEvent, render, screen} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {ViewportOverlayRoot} from 'isl-components/ViewportOverlay';
import DebugToolsMenu from '../debug/DebugToolsMenu';
import platform from '../platform';
import {resetTestMessages, simulateMessageFromServer} from '../testUtils';

describe('DebugToolsMenu', () => {
  const originalRequestIdleCallback = globalThis.requestIdleCallback;

  beforeEach(() => {
    resetTestMessages();
    globalThis.requestIdleCallback = jest.fn(() => 0);
    render(<ViewportOverlayRoot />);
  });

  afterEach(() => {
    globalThis.requestIdleCallback = originalRequestIdleCallback;
    jest.restoreAllMocks();
  });

  it('shows and copies the available debug log file path', () => {
    const logFilePath = '/tmp/isl-server-log/session/isl-server.log';
    const copySpy = jest.spyOn(platform, 'clipboardCopy').mockImplementation(() => {});
    render(<DebugToolsMenu dismiss={() => {}} />);

    act(() => {
      simulateMessageFromServer({
        type: 'applicationInfo',
        info: {
          platformName: 'browser',
          version: '1.2.3-test',
          logFilePath,
        },
      });
    });

    expect(screen.getByText('Debug log')).toBeInTheDocument();
    const copyablePath = screen.getByText(logFilePath);
    expect(copyablePath).toBeInTheDocument();

    fireEvent.click(copyablePath);
    expect(copySpy).toHaveBeenCalledWith(logFilePath);
  });

  it('copies the available debug log file path with the keyboard', () => {
    const logFilePath = '/tmp/isl-server-log/session/isl-server.log';
    const copySpy = jest.spyOn(platform, 'clipboardCopy').mockImplementation(() => {});
    render(<DebugToolsMenu dismiss={() => {}} />);

    act(() => {
      simulateMessageFromServer({
        type: 'applicationInfo',
        info: {
          platformName: 'browser',
          version: '1.2.3-test',
          logFilePath,
        },
      });
    });

    screen.getByText(logFilePath).focus();
    userEvent.keyboard('{Enter}');

    expect(copySpy).toHaveBeenCalledWith(logFilePath);
  });

  it('shows when no debug log file path is available', () => {
    render(<DebugToolsMenu dismiss={() => {}} />);

    act(() => {
      simulateMessageFromServer({
        type: 'applicationInfo',
        info: {
          platformName: 'vscode',
          version: '1.2.3-test',
        },
      });
    });

    expect(screen.getByText('Debug log')).toBeInTheDocument();
    expect(screen.getByText('Not available')).toBeInTheDocument();
  });
});
