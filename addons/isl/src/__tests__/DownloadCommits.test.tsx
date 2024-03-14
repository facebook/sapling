/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import {
  closeCommitInfoSidebar,
  COMMIT,
  expectMessageSentToServer,
  resetTestMessages,
  simulateCommits,
  simulateMessageFromServer,
  TEST_COMMIT_HISTORY,
} from '../testUtils';
import {CommandRunner} from '../types';
import {screen, act, render, fireEvent, waitFor} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import * as utils from 'shared/utils';

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

  it('supports goto', async () => {
    fireEvent.click(screen.getByTestId('download-commits-tooltip-button'));

    act(() => {
      userEvent.type(screen.getByTestId('download-commits-input'), 'aaaaaa');
      fireEvent.click(screen.getByText('Go to'));
      fireEvent.click(screen.getByText('Rebase to Stack Base'));
    });

    fireEvent.click(screen.getByTestId('download-commit-button'));

    jest.spyOn(utils, 'randomId').mockImplementationOnce(() => '111');
    expectMessageSentToServer({
      type: 'runOperation',
      operation: {
        args: ['pull', '--rev', {type: 'exact-revset', revset: 'aaaaaa'}],
        runner: CommandRunner.Sapling,
        trackEventName: 'PullRevOperation',
        id: expect.anything(),
      },
    });
    act(() =>
      simulateMessageFromServer({
        type: 'operationProgress',
        id: '111',
        kind: 'exit',
        exitCode: 0,
        timestamp: 0,
      }),
    );
    await waitFor(() => {
      expectMessageSentToServer({
        type: 'fetchLatestCommit',
        revset: 'aaaaaa',
      });
    });
    act(() =>
      simulateMessageFromServer({
        type: 'fetchedLatestCommit',
        revset: 'aaaaaa',
        info: {value: COMMIT('aaaaaa', 'Commit A', '0', {phase: 'draft'})},
      }),
    );
    await waitFor(() =>
      expectMessageSentToServer({
        type: 'runOperation',
        operation: {
          args: [
            'rebase',
            '-s',
            {type: 'exact-revset', revset: 'aaaaaa'},
            '-d',
            {type: 'succeedable-revset', revset: '1'},
          ],
          runner: CommandRunner.Sapling,
          trackEventName: 'RebaseOperation',
          id: expect.anything(),
        },
      }),
    );
    expectMessageSentToServer({
      type: 'runOperation',
      operation: {
        args: ['goto', '--rev', {type: 'succeedable-revset', revset: 'aaaaaa'}],
        runner: CommandRunner.Sapling,
        trackEventName: 'GotoOperation',
        id: expect.anything(),
      },
    });
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
