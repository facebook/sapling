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
import {fireEvent, render, screen} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {act} from 'react-dom/test-utils';
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
          args: ['shelve'],
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
          args: ['shelve', '--name', 'My Shelf'],
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
});
