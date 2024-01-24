/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import {
  closeCommitInfoSidebar,
  expectMessageSentToServer,
  resetTestMessages,
  simulateCommits,
  TEST_COMMIT_HISTORY,
} from '../testUtils';
import {screen, act, render, fireEvent} from '@testing-library/react';
import userEvent from '@testing-library/user-event';

jest.mock('../MessageBus');

describe('Download Commits', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);

    act(() => {
      closeCommitInfoSidebar();
      simulateCommits({value: TEST_COMMIT_HISTORY});
    });
  });

  it('starts focused', () => {
    fireEvent.click(screen.getByTestId('download-commits-tooltip-button'));

    expect(screen.getByTestId('download-commits-input')).toHaveFocus();
  });

  it('runs operation', () => {
    fireEvent.click(screen.getByTestId('download-commits-tooltip-button'));

    act(() => {
      userEvent.type(screen.getByTestId('download-commits-input'), 'aaaaaa');
    });

    fireEvent.click(screen.getByTestId('download-commit-button'));
    expectMessageSentToServer(
      expect.objectContaining({
        type: 'runOperation',
      }),
    );
  });

  it('keyboard shortcut support', () => {
    fireEvent.click(screen.getByTestId('download-commits-tooltip-button'));

    act(() => {
      userEvent.type(screen.getByTestId('download-commits-input'), '{cmd}aaa{enter}{/cmd}');
    });

    expectMessageSentToServer(
      expect.objectContaining({
        type: 'runOperation',
        operation: expect.objectContaining({
          args: expect.arrayContaining([{type: 'exact-revset', revset: 'aaa'}]),
        }),
      }),
    );
  });
});
