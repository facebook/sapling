/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  simulateUncommittedChangedFiles,
  closeCommitInfoSidebar,
  simulateMessageFromServer,
} from '../testUtils';
import {render, screen} from '@testing-library/react';
import {act} from 'react-dom/test-utils';

jest.mock('../MessageBus');

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
          COMMIT('b', 'Another Commit', 'a', {isHead: true}),
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
        type: 'subscribeMergeConflicts',
        subscriptionID: expect.anything(),
      });
      simulateMessageFromServer({
        type: 'mergeConflicts',
        subscriptionID: 'latestUncommittedChanges',
        conflicts: {
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
          type: 'mergeConflicts',
          subscriptionID: 'latestUncommittedChanges',
          conflicts: {
            state: 'loaded',
            command: 'rebase',
            toContinue: 'rebase --continue',
            toAbort: 'rebase --abort',
            files: [
              {path: 'src/file2.js', status: 'U'},
              {path: 'src/file3.js', status: 'Resolved'},
            ],
          },
        });
      });
    });

    it('renders merge conflicts changes', () => {
      expect(screen.getByText('src/file2.js', {exact: false})).toBeInTheDocument();
      expect(screen.getByText('src/file3.js', {exact: false})).toBeInTheDocument();

      // uncommitted changes are not there
      expect(screen.queryByText('src/file1.js', {exact: false})).not.toBeInTheDocument();
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
          type: 'mergeConflicts',
          subscriptionID: 'latestUncommittedChanges',
          conflicts: {
            state: 'loaded',
            command: 'rebase',
            toContinue: 'rebase --continue',
            toAbort: 'rebase --abort',
            files: [
              {path: 'src/file2.js', status: 'Resolved'},
              {path: 'src/file3.js', status: 'Resolved'},
            ],
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
  });
});
