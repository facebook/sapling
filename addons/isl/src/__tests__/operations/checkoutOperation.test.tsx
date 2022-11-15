/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../../types';

import App from '../../App';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  closeCommitInfoSidebar,
} from '../../testUtils';
import {CommandRunner, SucceedableRevset} from '../../types';
import {fireEvent, render, screen, within} from '@testing-library/react';
import {act} from 'react-dom/test-utils';

jest.mock('../../MessageBus');

describe('GotoOperation', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);
    act(() => {
      closeCommitInfoSidebar();
      expectMessageSentToServer({
        type: 'subscribeSmartlogCommits',
        subscriptionID: expect.anything(),
      });
      simulateCommits({
        value: [
          COMMIT('2', 'master', '00', {phase: 'public', remoteBookmarks: ['remote/master']}),
          COMMIT('1', 'Commit 1', '0', {phase: 'public'}),
          COMMIT('a', 'Commit A', '1'),
          COMMIT('b', 'Commit B', 'a', {isHead: true}),
          COMMIT('c', 'Commit C', 'b'),
        ],
      });
    });
  });

  const clickGoto = (commit: Hash) => {
    const myCommit = screen.queryByTestId(`commit-${commit}`);
    const gotoButton = myCommit?.querySelector('.goto-button button');
    expect(gotoButton).toBeDefined();
    fireEvent.click(gotoButton as Element);
  };

  it('runs goto', () => {
    clickGoto('a');

    expectMessageSentToServer({
      type: 'runOperation',
      operation: {
        args: ['goto', '--rev', SucceedableRevset('a')],
        id: expect.anything(),
        runner: CommandRunner.Sapling,
      },
    });
  });

  it('renders optimistic state while running', () => {
    clickGoto('a');

    expect(
      within(screen.getByTestId('commit-a')).queryByText("You're moving here..."),
    ).toBeInTheDocument();
    expect(
      within(screen.getByTestId('commit-b')).queryByText('You were here...'),
    ).toBeInTheDocument();
  });

  it('optimistic state resolves after goto completes', () => {
    clickGoto('a');

    act(() => {
      simulateCommits({
        value: [
          COMMIT('1', 'Commit 1', '0', {phase: 'public'}),
          COMMIT('a', 'Commit A', '1', {isHead: true}),
          COMMIT('b', 'Commit B', 'a'),
          COMMIT('c', 'Commit C', 'b'),
        ],
      });
    });

    expect(within(screen.getByTestId('commit-a')).queryByText('You are here')).toBeInTheDocument();
    expect(screen.queryByText("You're moving here...")).not.toBeInTheDocument();
    expect(screen.queryByText('You were here...')).not.toBeInTheDocument();
  });

  describe('bookmarks as destinations', () => {
    it('runs goto with bookmark', () => {
      clickGoto('2');

      expectMessageSentToServer({
        type: 'runOperation',
        operation: {
          args: ['goto', '--rev', SucceedableRevset('remote/master')],
          id: expect.anything(),
          runner: CommandRunner.Sapling,
        },
      });
    });

    it('renders optimistic state while running', () => {
      clickGoto('2');

      expect(
        within(screen.getByTestId('commit-2')).queryByText("You're moving here..."),
      ).toBeInTheDocument();
      expect(
        within(screen.getByTestId('commit-b')).queryByText('You were here...'),
      ).toBeInTheDocument();
    });
  });
});
