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
  simulateRepoConnected,
} from '../testUtils';
import {CommandRunner} from '../types';
import {fireEvent, render, screen, waitFor} from '@testing-library/react';
import {act} from 'react-dom/test-utils';

jest.mock('../MessageBus');

describe('CommitTreeList', () => {
  beforeEach(() => {
    resetTestMessages();
  });

  it('shows loading spinner on mount', () => {
    render(<App />);

    expect(screen.getByTestId('loading-spinner')).toBeInTheDocument();
  });

  describe('after commits loaded', () => {
    beforeEach(() => {
      render(<App />);
      act(() => {
        simulateRepoConnected();
        closeCommitInfoSidebar();
        expectMessageSentToServer({
          type: 'subscribeSmartlogCommits',
          subscriptionID: expect.anything(),
        });
        simulateCommits({
          value: [
            COMMIT('1', 'some public base', '0', {phase: 'public'}),
            COMMIT('a', 'My Commit', '1'),
            COMMIT('b', 'Another Commit', 'a', {isHead: true}),
          ],
        });
      });
    });

    it('renders commits', () => {
      expect(screen.getByText('My Commit')).toBeInTheDocument();
      expect(screen.getByText('Another Commit')).toBeInTheDocument();
      expect(screen.queryByText('some public base')).not.toBeInTheDocument();
    });

    it('renders exactly one head', () => {
      expect(screen.getByText('You are here')).toBeInTheDocument();
    });

    describe('uncommitted changes', () => {
      beforeEach(() => {
        act(() => {
          expectMessageSentToServer({
            type: 'subscribeUncommittedChanges',
            subscriptionID: expect.anything(),
          });
          simulateUncommittedChangedFiles({
            value: [
              {path: 'src/file.js', status: 'M'},
              {path: 'src/file_add.js', status: 'A'},
              {path: 'src/file_removed.js', status: 'R'},
              {path: 'src/file_untracked.js', status: '?'},
              {path: 'src/file_missing.js', status: '!'},
            ],
          });
        });
      });

      it('renders uncommitted changes', () => {
        expect(screen.getByText('src/file.js', {exact: false})).toBeInTheDocument();
        expect(screen.getByText('src/file_add.js', {exact: false})).toBeInTheDocument();
        expect(screen.getByText('src/file_removed.js', {exact: false})).toBeInTheDocument();
        expect(screen.getByText('src/file_untracked.js', {exact: false})).toBeInTheDocument();
        expect(screen.getByText('src/file_missing.js', {exact: false})).toBeInTheDocument();
      });

      it('shows file actions', () => {
        const fileActions = screen.getAllByTestId('file-actions');
        expect(fileActions).toHaveLength(5);
        const revertButtons = screen.getAllByTestId('file-revert-button');
        expect(revertButtons).toHaveLength(4); // modified, added, removed, missing
      });

      it('runs revert command when clicking revert button', async () => {
        const revertButtons = screen.getAllByTestId('file-revert-button');
        jest.spyOn(window, 'confirm').mockImplementation(() => true);
        act(() => {
          fireEvent.click(revertButtons[0]);
        });
        expect(window.confirm).toHaveBeenCalled();
        await waitFor(() => {
          expectMessageSentToServer({
            type: 'runOperation',
            operation: {
              args: ['revert', {path: 'src/file.js', type: 'repo-relative-file'}],
              id: expect.anything(),
              runner: CommandRunner.Sapling,
            },
          });
        });
      });
    });

    it('shows log errors', () => {
      act(() => {
        simulateCommits({
          error: new Error('error running log'),
        });
      });
      expect(screen.getByText('Failed to fetch commits')).toBeInTheDocument();
      expect(screen.getByText('error running log')).toBeInTheDocument();

      // we should still have commits from the last successful fetch
      expect(screen.getByText('My Commit')).toBeInTheDocument();
      expect(screen.getByText('Another Commit')).toBeInTheDocument();
      expect(screen.queryByText('some public base')).not.toBeInTheDocument();
    });

    it('shows status errors', () => {
      act(() => {
        simulateUncommittedChangedFiles({
          error: new Error('error running status'),
        });
      });
      expect(screen.getByText('Failed to fetch Uncommitted Changes')).toBeInTheDocument();
      expect(screen.getByText('error running status')).toBeInTheDocument();
    });

    it('shows successor info', () => {
      act(() => {
        simulateCommits({
          value: [
            COMMIT('1', 'some public base', '0', {phase: 'public'}),
            COMMIT('a', 'My Commit', '1', {successorInfo: {hash: 'a2', type: 'land'}}),
            COMMIT('b', 'Another Commit', 'a', {isHead: true}),
          ],
        });
      });
      expect(screen.getByText('Landed as a newer commit', {exact: false})).toBeInTheDocument();
    });
  });
});
