/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import {ignoreRTL} from '../testQueries';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  simulateUncommittedChangedFiles,
  closeCommitInfoSidebar,
  simulateRepoConnected,
  simulateMessageFromServer,
} from '../testUtils';
import {fireEvent, render, screen, waitFor, within} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {act} from 'react-dom/test-utils';
import {ComparisonType} from 'shared/Comparison';
import {unwrap} from 'shared/utils';

jest.mock('../MessageBus');

describe('Shelve', () => {
  beforeEach(() => {
    resetTestMessages();
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

      simulateUncommittedChangedFiles({
        value: [
          {path: 'src/file1.js', status: 'M'},
          {path: 'src/file2.js', status: 'M'},
          {path: 'src/file3.js', status: 'M'},
        ],
      });
    });
  });

  describe('Shelve button', () => {
    it('Runs shelve when clicking button', () => {
      const shelveButton = screen.getByText('Shelve');
      expect(shelveButton).toBeInTheDocument();

      fireEvent.click(shelveButton);
      expectMessageSentToServer({
        type: 'runOperation',
        operation: expect.objectContaining({
          args: ['shelve', '--unknown'],
        }),
      });
    });

    it('Optimistic state hides all files when shelve running', () => {
      expect(screen.queryAllByTestId(/changed-file-src\/file.\.js/)).toHaveLength(3);
      const shelveButton = screen.getByText('Shelve');
      fireEvent.click(shelveButton);
      expect(screen.queryAllByTestId(/changed-file-src\/file.\.js/)).toHaveLength(0);
    });

    it('includes name for shelved change if typed', () => {
      const shelveButton = screen.getByText('Shelve');
      expect(shelveButton).toBeInTheDocument();

      const quickInput = screen.getByTestId('quick-commit-title');

      act(() => {
        userEvent.type(quickInput, 'My Shelf');
      });

      fireEvent.click(shelveButton);
      expectMessageSentToServer({
        type: 'runOperation',
        operation: expect.objectContaining({
          args: ['shelve', '--unknown', '--name', 'My Shelf'],
        }),
      });
    });

    it('only shelves selected files', () => {
      const shelveButton = screen.getByText('Shelve');
      expect(shelveButton).toBeInTheDocument();

      // uncheck one file
      fireEvent.click(
        unwrap(
          screen.getByTestId('changed-file-src/file2.js').querySelector('input[type=checkbox]'),
        ),
      );

      fireEvent.click(shelveButton);

      expectMessageSentToServer({
        type: 'runOperation',
        operation: expect.objectContaining({
          args: [
            'shelve',
            '--unknown',
            {type: 'repo-relative-file', path: 'src/file1.js'},
            {type: 'repo-relative-file', path: 'src/file3.js'},
          ],
        }),
      });
    });

    it('Optimistic state hides selected files when shelve running', () => {
      expect(screen.queryAllByTestId(/src\/file.\.js/)).toHaveLength(3);
      const shelveButton = screen.getByText('Shelve');
      fireEvent.click(shelveButton);
      expect(screen.queryAllByTestId(/src\/file.\.js/)).toHaveLength(0);
    });

    it('shelve button disabled if no files selected', () => {
      fireEvent.click(screen.getByText('Deselect All'));

      const shelveButton = screen.getByText('Shelve');
      expect(shelveButton).toBeInTheDocument();
      expect(shelveButton).toBeDisabled();
    });
  });

  describe('Shelved changes list', () => {
    const SHELVES = [
      {
        hash: 'aaa',
        name: 'my shelve',
        date: new Date(),
        description: 'shelved on commit b',
        filesSample: [{path: 'src/shelvedfile.js', status: 'M' as const}],
        totalFileCount: 1,
      },
    ];

    it('shows shelved changes list tooltip', () => {
      fireEvent.click(screen.getByTestId('shelved-changes-button'));
      expect(screen.getByText('Shelved Changes')).toBeInTheDocument();

      expectMessageSentToServer({
        type: 'fetchShelvedChanges',
      });
    });

    it('renders errors from fetching shelved changes', async () => {
      fireEvent.click(screen.getByTestId('shelved-changes-button'));
      act(() => {
        simulateMessageFromServer({
          type: 'fetchedShelvedChanges',
          shelvedChanges: {
            error: new Error('failed to fetch shelved changes'),
          },
        });
      });

      await waitFor(() => {
        expect(
          within(screen.getByTestId('shelved-changes-dropdown')).getByText(
            'Could not fetch shelved changes',
          ),
        ).toBeInTheDocument();
      });
    });

    it('renders empty state', async () => {
      fireEvent.click(screen.getByTestId('shelved-changes-button'));
      act(() => {
        simulateMessageFromServer({
          type: 'fetchedShelvedChanges',
          shelvedChanges: {
            value: [],
          },
        });
      });

      await waitFor(() => {
        expect(
          within(screen.getByTestId('shelved-changes-dropdown')).getByText('No shelved changes'),
        ).toBeInTheDocument();
      });
    });

    describe('with shelved change rendered', () => {
      beforeEach(async () => {
        fireEvent.click(screen.getByTestId('shelved-changes-button'));
        act(() => {
          simulateMessageFromServer({
            type: 'fetchedShelvedChanges',
            shelvedChanges: {
              value: SHELVES,
            },
          });
        });

        await waitFor(() => {
          expect(
            within(screen.getByTestId('shelved-changes-dropdown')).getByText('my shelve'),
          ).toBeInTheDocument();
        });
      });

      it('renders shelved changes', () => {
        expect(
          within(screen.getByTestId('shelved-changes-dropdown')).getByText(
            ignoreRTL('shelvedfile.js'),
          ),
        ).toBeInTheDocument();
      });

      it('runs unshelve', () => {
        fireEvent.click(screen.getByText('Unshelve'));

        expectMessageSentToServer({
          type: 'runOperation',
          operation: expect.objectContaining({
            args: ['unshelve', '--name', 'my shelve'],
          }),
        });
      });

      it('dismisses tooltip while running unshelve', () => {
        fireEvent.click(screen.getByText('Unshelve'));

        expect(screen.queryByTestId('shelved-changes-dropdown')).not.toBeInTheDocument();
      });

      it('can open comparison view for the shelve', async () => {
        fireEvent.click(
          within(screen.getByTestId('shelved-changes-dropdown')).getByText('View Changes'),
        );

        await waitFor(() => {
          expectMessageSentToServer({
            type: 'requestComparison',
            comparison: {
              type: ComparisonType.Committed,
              hash: 'aaa',
            },
          });
        });
      });

      it('refetches shelves when reopening dropdown', async () => {
        expect(
          within(screen.getByTestId('shelved-changes-dropdown')).getByText('my shelve'),
        ).toBeInTheDocument();

        resetTestMessages();

        act(() => {
          userEvent.type(document.body, '{Escape}');
        });

        // reopen the dropdown
        fireEvent.click(screen.getByTestId('shelved-changes-button'));
        expectMessageSentToServer({
          type: 'fetchShelvedChanges',
        });

        // send a new set of changes
        const NEW_SHELVES = [
          {
            hash: 'a',
            name: 'my new shelve',
            date: new Date(),
            description: 'shelved on commit b',
            filesSample: [{path: 'src/shelvedfile.js', status: 'M' as const}],
            totalFileCount: 1,
          },
        ];
        act(() => {
          simulateMessageFromServer({
            type: 'fetchedShelvedChanges',
            shelvedChanges: {
              value: NEW_SHELVES,
            },
          });
        });

        // make sure we get the new name and not the old one
        expect(
          within(screen.getByTestId('shelved-changes-dropdown')).queryByText('my shelve'),
        ).not.toBeInTheDocument();
        await waitFor(() => {
          expect(
            within(screen.getByTestId('shelved-changes-dropdown')).getByText('my new shelve'),
          ).toBeInTheDocument();
        });
      });

      it('shows optimistic state for unshelve', () => {
        expect(
          within(screen.getByTestId('commit-tree-root')).queryByText(ignoreRTL('shelvedfile.js')),
        ).not.toBeInTheDocument();

        fireEvent.click(screen.getByText('Unshelve'));

        expect(
          within(screen.getByTestId('commit-tree-root')).getByText(ignoreRTL('shelvedfile.js')),
        ).toBeInTheDocument();
      });
    });
  });
});
