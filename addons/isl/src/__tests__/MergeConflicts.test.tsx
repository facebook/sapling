/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import {mostRecentSubscriptionIds} from '../serverAPIState';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  simulateUncommittedChangedFiles,
  closeCommitInfoSidebar,
  simulateMessageFromServer,
} from '../testUtils';
import {fireEvent, render, screen, act} from '@testing-library/react';

describe('CommitTreeList', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);
    act(() => {
      closeCommitInfoSidebar();
      simulateCommits({
        value: [
          COMMIT('1', 'some public base', '0', {phase: 'public'}),
          COMMIT('a', 'My Commit', '1'),
          COMMIT('b', 'Another Commit', 'a', {isDot: true}),
        ],
      });
      simulateUncommittedChangedFiles({
        value: [
          {path: 'src/file1.js', status: 'M'},
          {path: 'src/file2.js', status: 'M'},
          {path: 'src/file3.js', status: 'M'},
        ],
      });
    });

    act(() => {
      expectMessageSentToServer({
        type: 'subscribe',
        kind: 'mergeConflicts',
        subscriptionID: expect.anything(),
      });
      simulateMessageFromServer({
        type: 'subscriptionResult',
        kind: 'mergeConflicts',
        subscriptionID: mostRecentSubscriptionIds.mergeConflicts,
        data: {
          state: 'loading',
        },
      });
    });
  });

  it('renders merge conflicts spinner while loading', () => {
    expect(screen.getByTestId('merge-conflicts-spinner')).toBeInTheDocument();
  });

  describe('after conflicts load', () => {
    beforeEach(() => {
      act(() => {
        simulateMessageFromServer({
          type: 'subscriptionResult',
          kind: 'mergeConflicts',
          subscriptionID: mostRecentSubscriptionIds.mergeConflicts,
          data: {
            state: 'loaded',
            command: 'rebase',
            toContinue: 'rebase --continue',
            toAbort: 'rebase --abort',
            files: [
              {path: 'src/file2.js', status: 'U'},
              {path: 'src/file3.js', status: 'Resolved'},
            ],
            fetchStartTimestamp: 1,
            fetchCompletedTimestamp: 2,
          },
        });
      });
    });

    it('renders merge conflicts changes', () => {
      expect(screen.getByText('file2.js', {exact: false})).toBeInTheDocument();
      expect(screen.getByText('file3.js', {exact: false})).toBeInTheDocument();

      // uncommitted changes are not there
      expect(screen.queryByText('file1.js', {exact: false})).not.toBeInTheDocument();
    });

    it("doesn't allow continue until conflicts resolved", () => {
      expect(
        screen.queryByText('All Merge Conflicts Resolved', {exact: false}),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByText('Resolve conflicts to continue rebase', {exact: false}),
      ).toBeInTheDocument();

      expect(
        (screen.queryByTestId('conflict-continue-button') as HTMLButtonElement).disabled,
      ).toEqual(true);

      act(() => {
        simulateMessageFromServer({
          type: 'subscriptionResult',
          kind: 'mergeConflicts',
          subscriptionID: mostRecentSubscriptionIds.mergeConflicts,
          data: {
            state: 'loaded',
            command: 'rebase',
            toContinue: 'rebase --continue',
            toAbort: 'rebase --abort',
            files: [
              {path: 'src/file2.js', status: 'Resolved'},
              {path: 'src/file3.js', status: 'Resolved'},
            ],
            fetchStartTimestamp: 1,
            fetchCompletedTimestamp: 2,
          },
        });
      });

      expect(
        screen.queryByText('All Merge Conflicts Resolved', {exact: false}),
      ).toBeInTheDocument();
      expect(
        screen.queryByText('Resolve conflicts to continue rebase', {exact: false}),
      ).not.toBeInTheDocument();

      expect((screen.queryByTestId('conflict-continue-button') as HTMLButtonElement).disabled).toBe(
        false,
      );
    });

    it('uses optimistic state to render resolved files', () => {
      const resolveButton = screen.getByTestId('file-action-resolve');
      act(() => {
        fireEvent.click(resolveButton);
      });
      // resolve button is no longer there
      expect(screen.queryByTestId('file-action-resolve')).not.toBeInTheDocument();
    });

    it('lets you continue when conflicts are optimistically resolved', () => {
      const resolveButton = screen.getByTestId('file-action-resolve');
      act(() => {
        fireEvent.click(resolveButton);
      });

      // continue is no longer disabled
      expect(
        (screen.queryByTestId('conflict-continue-button') as HTMLButtonElement).disabled,
      ).toEqual(false);
    });

    it('disables continue button while running', () => {
      const resolveButton = screen.getByTestId('file-action-resolve');
      act(() => {
        fireEvent.click(resolveButton);
      });
      const continueButton = screen.getByTestId('conflict-continue-button');
      act(() => {
        fireEvent.click(continueButton);
      });

      expect(
        (screen.queryByTestId('conflict-continue-button') as HTMLButtonElement).disabled,
      ).toEqual(true);

      expectMessageSentToServer({
        type: 'runOperation',
        operation: expect.objectContaining({args: ['continue']}),
      });

      // simulate continue finishing
      act(() => {
        simulateMessageFromServer({
          type: 'operationProgress',
          kind: 'exit',
          exitCode: 0,
          id: 'foo',
          timestamp: 1234,
        });
      });

      // We will soon get the next set of merge conflicts as null.
      // In the mean time, after `continue` has run, we still disable the button.
      expect(
        (screen.queryByTestId('conflict-continue-button') as HTMLButtonElement).disabled,
      ).toEqual(true);

      act(() => {
        simulateMessageFromServer({
          type: 'subscriptionResult',
          kind: 'mergeConflicts',
          subscriptionID: mostRecentSubscriptionIds.mergeConflicts,
          data: undefined,
        });
      });
      expect(screen.queryByTestId('conflict-continue-button')).not.toBeInTheDocument();
    });
  });
});
