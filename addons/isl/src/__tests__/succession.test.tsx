/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import {successionTracker} from '../SuccessionTracker';
import {CommitInfoTestUtils} from '../testQueries';
import {resetTestMessages, expectMessageSentToServer, simulateCommits, COMMIT} from '../testUtils';
import {render, act} from '@testing-library/react';
import userEvent from '@testing-library/user-event';

jest.mock('../MessageBus');

describe('succession', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);
    act(() => {
      expectMessageSentToServer({
        type: 'subscribe',
        kind: 'smartlogCommits',
        subscriptionID: expect.anything(),
      });
      simulateCommits({
        value: [
          COMMIT('1', 'Commit 1', '0', {phase: 'public'}),
          COMMIT('a', 'Commit A', '1'),
          COMMIT('b', 'Commit B', 'a', {isHead: true}),
          COMMIT('c', 'Commit C', 'b'),
        ],
      });
    });
  });
  afterEach(() => {
    successionTracker.clear();
  });

  describe('edited commit message', () => {
    it('uses succession to maintain edited commit message', () => {
      CommitInfoTestUtils.clickToEditTitle();
      CommitInfoTestUtils.clickToEditDescription();

      CommitInfoTestUtils.expectIsEditingTitle();
      CommitInfoTestUtils.expectIsEditingDescription();

      act(() => {
        userEvent.type(CommitInfoTestUtils.getTitleEditor(), ' modified!');
        userEvent.type(CommitInfoTestUtils.getDescriptionEditor(), 'my description');
      });

      act(() => {
        simulateCommits({
          value: [
            COMMIT('1', 'Commit 1', '0', {phase: 'public'}),
            COMMIT('a2', 'Commit A', '1', {closestPredecessors: ['a']}),
            COMMIT('b2', 'Commit B', 'a2', {isHead: true, closestPredecessors: ['b']}),
            COMMIT('c2', 'Commit C', 'b2', {closestPredecessors: ['c']}),
          ],
        });
      });

      CommitInfoTestUtils.expectIsEditingTitle();
      CommitInfoTestUtils.expectIsEditingDescription();

      expect(
        CommitInfoTestUtils.withinCommitInfo().getByText('Commit B modified!'),
      ).toBeInTheDocument();
      expect(
        CommitInfoTestUtils.withinCommitInfo().getByText('my description'),
      ).toBeInTheDocument();
    });
  });

  describe('commit selection state', () => {
    it('uses succession to maintain commit selection', () => {
      CommitInfoTestUtils.clickToSelectCommit('c');

      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit C')).toBeInTheDocument();

      act(() => {
        simulateCommits({
          value: [
            COMMIT('1', 'Commit 1', '0', {phase: 'public'}),
            COMMIT('a2', 'Commit A', '1', {closestPredecessors: ['a']}),
            COMMIT('b2', 'Commit B', 'a2', {isHead: true, closestPredecessors: ['b']}),
            COMMIT('c2', 'Commit C', 'b2', {closestPredecessors: ['c']}),
          ],
        });
      });

      // Commit C is still selected, even though its hash changed
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit C')).toBeInTheDocument();
    });
  });
});
