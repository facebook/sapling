/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import {__TEST__} from '../CommitInfoView/CommitInfoState';
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
    __TEST__.renewEditedCommitMessageSuccessionSubscription();
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

    it('bug: does not propagate optimistic state message', () => {
      // load a set of commits with hash A as head. (without any edited message for A)
      // load a new set of commits, with hash A succeeded by hash A2.
      // ensure commit info view is editable.

      act(() => {
        simulateCommits({
          value: [
            COMMIT('1', 'Commit 1', '0', {phase: 'public'}),
            COMMIT('x', 'Commit X', '1', {isHead: true}),
          ],
        });
      });
      act(() => {
        simulateCommits({
          value: [
            COMMIT('1', 'Commit 1', '0', {phase: 'public'}),
            COMMIT('x2', 'Commit X2', '1', {isHead: true, closestPredecessors: ['x']}),
          ],
        });
      });

      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit X2')).toBeInTheDocument();

      // Resulting commit being viewed should be editable: clicking the edit buttons work.
      CommitInfoTestUtils.clickToEditTitle();
      CommitInfoTestUtils.clickToEditDescription();
      CommitInfoTestUtils.expectIsEditingTitle();
      CommitInfoTestUtils.expectIsEditingDescription();
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
