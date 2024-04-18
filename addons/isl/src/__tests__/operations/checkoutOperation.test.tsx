/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../../types';

import App from '../../App';
import platform from '../../platform';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  closeCommitInfoSidebar,
  expectYouAreHerePointAt,
  expectMessageNOTSentToServer,
} from '../../testUtils';
import {CommandRunner, succeedableRevset} from '../../types';
import {fireEvent, render, screen, act} from '@testing-library/react';
import {nextTick} from 'shared/testUtils';

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

  const clickGoto = async (commit: Hash) => {
    const myCommit = screen.queryByTestId(`commit-${commit}`);
    const gotoButton = myCommit?.querySelector('.goto-button button');
    expect(gotoButton).toBeDefined();
    await act(async () => {
      fireEvent.click(gotoButton as Element);
      await nextTick(); // async check if commit is too old
    });
  };

  it('goto button is accessible', () => {
    expect(screen.getByLabelText('Go to commit "Commit A"')).toBeInTheDocument();
    expect(screen.queryByLabelText('Go to commit "Commit B"')).not.toBeInTheDocument(); // already head, no goto button
    expect(screen.getByLabelText('Go to commit "Commit C"')).toBeInTheDocument();
  });

  it('runs goto', async () => {
    await clickGoto('a');

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

  it('renders optimistic state while running', async () => {
    await clickGoto('a');

    expectYouAreHerePointAt('a');
  });

  it('optimistic state resolves after goto completes', async () => {
    await clickGoto('a');

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
    it('runs goto with bookmark', async () => {
      await clickGoto('2');

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

    it('renders optimistic state while running', async () => {
      await clickGoto('2');

      expectYouAreHerePointAt('2');
    });
  });

  describe('succession', () => {
    it('handles successions', async () => {
      await clickGoto('c');

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

  describe('age warning', () => {
    let confirmSpy: jest.SpyInstance;
    beforeEach(() => {
      confirmSpy = jest.spyOn(platform, 'confirm').mockImplementation(() => Promise.resolve(true));
      act(() => {
        simulateCommits({
          value: [
            COMMIT('b', 'Commit B', 'a', {isDot: true, date: new Date('2024-03-04')}),
            COMMIT('a', 'Commit A', '3', {date: new Date('2024-03-03')}),
            COMMIT('3', 'Commit 3', '003', {phase: 'public', date: new Date('2024-03-02')}),
            COMMIT('2', 'Commit 2', '002', {phase: 'public', date: new Date('2024-03-01')}),
            COMMIT('x', 'Commit X', '1', {date: new Date('2024-03-03')}),
            COMMIT('1', 'Commit 1', '001', {phase: 'public', date: new Date('2020-01-01')}),
          ],
        });
      });
    });

    it('warns if going to an old commit', async () => {
      await clickGoto('1');
      expect(confirmSpy).toHaveBeenCalled();
    });

    it("cancels goto if you don't confirm", async () => {
      confirmSpy = jest.spyOn(platform, 'confirm').mockImplementation(() => Promise.resolve(false));
      await clickGoto('1');
      expect(confirmSpy).toHaveBeenCalled();
      expectMessageNOTSentToServer({
        type: 'runOperation',
        operation: expect.objectContaining({
          args: expect.arrayContaining(['goto']),
        }),
      });
    });

    it('does not warn for short goto', async () => {
      await clickGoto('a');
      expect(confirmSpy).not.toHaveBeenCalled();
    });

    it('compares base public commit, not destination itself', async () => {
      await clickGoto('x'); // x is only 1 day old, but its parent is months older than b's public base.
      expect(confirmSpy).toHaveBeenCalled();
    });

    it('only warns going backwards, not forwards', async () => {
      act(() => {
        simulateCommits({
          value: [
            COMMIT('b', 'Commit B', 'a', {date: new Date('2024-03-04')}),
            COMMIT('a', 'Commit A', '3', {date: new Date('2024-03-03')}),
            COMMIT('3', 'Commit 3', '003', {phase: 'public', date: new Date('2024-03-02')}),
            COMMIT('2', 'Commit 2', '002', {phase: 'public', date: new Date('2024-03-01')}),
            COMMIT('x', 'Commit X', '1', {isDot: true, date: new Date('2024-03-03')}),
            COMMIT('1', 'Commit 1', '001', {phase: 'public', date: new Date('2020-01-01')}),
          ],
        });
      });
      await clickGoto('b');
      expect(confirmSpy).not.toHaveBeenCalled();
    });
  });
});
