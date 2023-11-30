/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import {CommitInfoTestUtils, ignoreRTL} from '../testQueries';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  simulateUncommittedChangedFiles,
  closeCommitInfoSidebar,
  simulateRepoConnected,
  commitInfoIsOpen,
} from '../testUtils';
import {CommandRunner} from '../types';
import {fireEvent, render, screen, waitFor, within} from '@testing-library/react';
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

  it('shows bug button even on error', () => {
    render(<App />);

    act(() => {
      simulateCommits({
        error: new Error('invalid certificate'),
      });
    });

    expect(screen.getByTestId('bug-button')).toBeInTheDocument();
    expect(screen.getByText('invalid certificate')).toBeInTheDocument();
  });

  describe('after commits loaded', () => {
    beforeEach(() => {
      render(<App />);
      act(() => {
        simulateRepoConnected();
        closeCommitInfoSidebar();
        expectMessageSentToServer({
          type: 'subscribe',
          kind: 'smartlogCommits',
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
            type: 'subscribe',
            kind: 'uncommittedChanges',
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
        expect(screen.getByText(ignoreRTL('file.js'), {exact: false})).toBeInTheDocument();
        expect(screen.getByText(ignoreRTL('file_add.js'), {exact: false})).toBeInTheDocument();
        expect(screen.getByText(ignoreRTL('file_removed.js'), {exact: false})).toBeInTheDocument();
        expect(
          screen.getByText(ignoreRTL('file_untracked.js'), {exact: false}),
        ).toBeInTheDocument();
        expect(screen.getByText(ignoreRTL('file_missing.js'), {exact: false})).toBeInTheDocument();
      });

      it('shows quick commit button', () => {
        expect(screen.getByText('Commit')).toBeInTheDocument();
      });
      it('shows quick amend button only on non-public commits', () => {
        expect(screen.getByText('Amend')).toBeInTheDocument();
        // checkout a public commit
        act(() => {
          simulateCommits({
            value: [
              COMMIT('1', 'some public base', '0', {phase: 'public', isHead: true}),
              COMMIT('a', 'My Commit', '1', {successorInfo: {hash: 'a2', type: 'land'}}),
              COMMIT('b', 'Another Commit', 'a'),
            ],
          });
        });
        // no longer see quick amend
        expect(screen.queryByText('Amend')).not.toBeInTheDocument();
      });

      it('shows file actions', () => {
        const fileActions = screen.getAllByTestId('file-actions');
        expect(fileActions).toHaveLength(5); // 5 files
        const revertButtons = screen.getAllByTestId('file-revert-button');
        expect(revertButtons).toHaveLength(3); // modified, removed, missing files can be reverted
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
              trackEventName: 'RevertOperation',
            },
          });
        });
      });

      describe('addremove', () => {
        it('hides addremove if all files tracked', () => {
          act(() => {
            simulateUncommittedChangedFiles({
              value: [
                {path: 'src/file.js', status: 'M'},
                {path: 'src/file_add.js', status: 'A'},
                {path: 'src/file_removed.js', status: 'R'},
              ],
            });
          });
          expect(screen.queryByTestId('addremove-button')).not.toBeInTheDocument();

          act(() => {
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
          expect(screen.queryByTestId('addremove-button')).toBeInTheDocument();
        });

        it('runs addremove', async () => {
          const addremove = screen.getByTestId('addremove-button');
          act(() => {
            fireEvent.click(addremove);
          });
          await waitFor(() => {
            expectMessageSentToServer({
              type: 'runOperation',
              operation: {
                args: ['addremove'],
                id: expect.anything(),
                runner: CommandRunner.Sapling,
                trackEventName: 'AddRemoveOperation',
              },
            });
          });
        });

        it('optimistically updates file statuses while addremove is running', async () => {
          const addremove = screen.getByTestId('addremove-button');
          act(() => {
            fireEvent.click(addremove);
          });
          await waitFor(() => {
            expectMessageSentToServer({
              type: 'runOperation',
              operation: {
                args: ['addremove'],
                id: expect.anything(),
                runner: CommandRunner.Sapling,
                trackEventName: 'AddRemoveOperation',
              },
            });
          });

          expect(
            document.querySelectorAll('.changed-files .changed-file.file-ignored'),
          ).toHaveLength(0);
        });

        it('runs addremove only on selected files that are untracked', async () => {
          const ignoredFileCheckboxes = document.querySelectorAll(
            '.changed-files .changed-file.file-ignored input[type="checkbox"]',
          );
          const missingFileCheckboxes = document.querySelectorAll(
            '.changed-files .changed-file.file-missing input[type="checkbox"]',
          );
          expect(ignoredFileCheckboxes).toHaveLength(1); // file_untracked.js
          expect(missingFileCheckboxes).toHaveLength(1); // file_missing.js
          act(() => {
            fireEvent.click(missingFileCheckboxes[0]);
          });

          const addremove = screen.getByTestId('addremove-button');
          act(() => {
            fireEvent.click(addremove);
          });
          await waitFor(() => {
            expectMessageSentToServer({
              type: 'runOperation',
              operation: {
                // note: although src/file.js & others are selected, they aren't passed to addremove as they aren't untracked
                args: ['addremove', {path: 'src/file_untracked.js', type: 'repo-relative-file'}],
                id: expect.anything(),
                runner: CommandRunner.Sapling,
                trackEventName: 'AddRemoveOperation',
              },
            });
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

    it('shows button to open sidebar', () => {
      act(() => {
        simulateCommits({
          value: [
            COMMIT('1', 'some public base', '0', {phase: 'public'}),
            COMMIT('a', 'Commit A', '1', {isHead: true}),
            COMMIT('b', 'Commit B', '1'),
          ],
        });
      });
      expect(commitInfoIsOpen()).toBeFalsy();

      // doesn't appear for public commits
      expect(
        within(screen.getByTestId('commit-1')).queryByTestId('open-commit-info-button'),
      ).not.toBeInTheDocument();

      const openButton = within(screen.getByTestId('commit-b')).getByTestId(
        'open-commit-info-button',
      );
      expect(openButton).toBeInTheDocument();
      // screen reader accessible
      expect(screen.getByLabelText('Open commit "Commit B"')).toBeInTheDocument();
      fireEvent.click(openButton);
      expect(commitInfoIsOpen()).toBeTruthy();
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit B')).toBeInTheDocument();
    });
  });
});
