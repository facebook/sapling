/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {act, fireEvent, render, screen, waitFor, within} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import App from '../App';
import {CommitInfoTestUtils} from '../testQueries';
import {
  closeCommitInfoSidebar,
  COMMIT,
  expectMessageSentToServer,
  resetTestMessages,
  simulateCommits,
  simulateUncommittedChangedFiles,
} from '../testUtils';

describe('Optimistic Revset', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);
    act(() => {
      expectMessageSentToServer({
        type: 'subscribe',
        kind: 'smartlogCommits',
        subscriptionID: expect.anything(),
      });
      simulateUncommittedChangedFiles({
        value: [
          {path: 'file1.txt', status: 'M'},
          {path: 'file2.txt', status: 'A'},
          {path: 'file3.txt', status: 'R'},
        ],
      });
      simulateCommits({
        value: [
          COMMIT('2', 'master', '00', {phase: 'public', remoteBookmarks: ['remote/master']}),
          COMMIT('1', 'Commit 1', '0', {phase: 'public'}),
          COMMIT('a', 'Commit A', '1'),
          COMMIT('b', 'Commit B', 'a', {isDot: true}),
        ],
      });
    });
  });

  const clickQuickCommit = async () => {
    const quickCommitButton = screen.getByTestId('quick-commit-button');
    act(() => {
      fireEvent.click(quickCommitButton);
    });
    await waitFor(() =>
      expectMessageSentToServer({
        type: 'runOperation',
        operation: expect.objectContaining({
          args: expect.arrayContaining(['commit']),
        }),
      }),
    );
  };

  function rightClickAndChooseFromContextMenu(element: Element, choiceMatcher: string) {
    act(() => {
      fireEvent.contextMenu(element);
    });
    const choice = within(screen.getByTestId('context-menu-container')).getByText(choiceMatcher);
    expect(choice).not.toEqual(null);
    act(() => {
      fireEvent.click(choice);
    });
  }

  it('after commit, uses revset to act on optimistic commit', async () => {
    act(() => {
      CommitInfoTestUtils.clickAmendMode();
      closeCommitInfoSidebar();
    });

    const mockDate = new Date('2024-01-01T00:00:00.000Z');
    jest.spyOn(Date, 'now').mockImplementation(() => {
      return mockDate.valueOf();
    });

    const quickInput = screen.getByTestId('quick-commit-title');
    act(() => {
      userEvent.type(quickInput, 'My Commit');
    });
    await clickQuickCommit();
    expectMessageSentToServer({
      type: 'runOperation',
      operation: expect.objectContaining({
        args: ['commit', '--addremove', '--message', 'My Commit'],
      }),
    });

    await waitFor(() => {
      expect(screen.getByText('My Commit')).toBeInTheDocument();
    });

    rightClickAndChooseFromContextMenu(screen.getByText('My Commit'), 'Hide Commit');
    fireEvent.click(screen.getByText('Hide'));

    expectMessageSentToServer({
      type: 'runOperation',
      operation: expect.objectContaining({
        args: [
          'hide',
          '--rev',
          {
            type: 'optimistic-revset',
            revset: 'first(sort((children(b)-b) & date(">Mon, 01 Jan 2024 00:00:00 GMT"),date))',
            fake: 'OPTIMISTIC_COMMIT_b',
          },
        ],
      }),
    });
  });
});
