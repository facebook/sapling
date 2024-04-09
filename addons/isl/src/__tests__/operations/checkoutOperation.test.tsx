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
  expectYouAreHerePointAt,
} from '../../testUtils';
import {CommandRunner, succeedableRevset} from '../../types';
import {fireEvent, render, screen, act} from '@testing-library/react';

describe('GotoOperation', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);
    act(() => {
      closeCommitInfoSidebar();
      expectMessageSentToServer({
        type: 'subscribe',
        kind: 'smartlogCommits',
        subscriptionID: expect.anything(),
      });
      simulateCommits({
        value: [
          COMMIT('2', 'master', '00', {phase: 'public', remoteBookmarks: ['remote/master']}),
          COMMIT('1', 'Commit 1', '0', {phase: 'public'}),
          COMMIT('a', 'Commit A', '1'),
          COMMIT('b', 'Commit B', 'a', {isDot: true}),
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

  it('goto button is accessible', () => {
    expect(screen.getByLabelText('Go to commit "Commit A"')).toBeInTheDocument();
    expect(screen.queryByLabelText('Go to commit "Commit B"')).not.toBeInTheDocument(); // already head, no goto button
    expect(screen.getByLabelText('Go to commit "Commit C"')).toBeInTheDocument();
  });

  it('runs goto', () => {
    clickGoto('a');

    expectMessageSentToServer({
      type: 'runOperation',
      operation: {
        args: ['goto', '--rev', succeedableRevset('a')],
        id: expect.anything(),
        runner: CommandRunner.Sapling,
        trackEventName: 'GotoOperation',
      },
    });
  });

  it('renders optimistic state while running', () => {
    clickGoto('a');

    expectYouAreHerePointAt('a');
  });

  it('optimistic state resolves after goto completes', () => {
    clickGoto('a');

    act(() => {
      simulateCommits({
        value: [
          COMMIT('1', 'Commit 1', '0', {phase: 'public'}),
          COMMIT('a', 'Commit A', '1', {isDot: true}),
          COMMIT('b', 'Commit B', 'a'),
          COMMIT('c', 'Commit C', 'b'),
        ],
      });
    });

    // With the DAG renderer, we no longer show old "were here" and new "moving here".
    // The idea is that there would be a spinner on the "status" calculation to indicate
    // the in-progress checkout. For now this test looks the same as the above.
    expectYouAreHerePointAt('a');
  });

  describe('bookmarks as destinations', () => {
    it('runs goto with bookmark', () => {
      clickGoto('2');

      expectMessageSentToServer({
        type: 'runOperation',
        operation: {
          args: ['goto', '--rev', succeedableRevset('remote/master')],
          id: expect.anything(),
          runner: CommandRunner.Sapling,
          trackEventName: 'GotoOperation',
        },
      });
    });

    it('renders optimistic state while running', () => {
      clickGoto('2');

      expectYouAreHerePointAt('2');
    });
  });

  describe('succession', () => {
    it('handles successions', () => {
      clickGoto('c');

      // get a new batch of commits from some other operation like rebase, which
      // rewrites a,b,c into a1,b2,c2
      act(() => {
        simulateCommits({
          value: [
            COMMIT('2', 'master', '00', {phase: 'public', remoteBookmarks: ['remote/master']}),
            COMMIT('1', 'Commit 1', '0', {phase: 'public'}),
            COMMIT('a2', 'Commit A', '1', {closestPredecessors: ['a']}),
            COMMIT('b2', 'Commit B', 'a2', {isDot: true, closestPredecessors: ['b']}),
            COMMIT('c2', 'Commit C', 'b2', {closestPredecessors: ['c']}),
          ],
        });
      });

      // "c" becomes "c2"
      expectYouAreHerePointAt('c2');
    });
  });
});
