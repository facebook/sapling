/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {act, fireEvent, render, screen, waitFor} from '@testing-library/react';
import {nextTick} from 'shared/testUtils';
import App from '../App';
import {
  closeCommitInfoSidebar,
  COMMIT,
  expectMessageNOTSentToServer,
  expectMessageSentToServer,
  resetTestMessages,
  simulateCommits,
  simulateMessageFromServer,
  simulateRepoConnected,
  simulateUncommittedChangedFiles,
} from '../testUtils';

describe('UnsavedFiles', () => {
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
          COMMIT('b', 'Another Commit', 'a', {isDot: true}),
        ],
      });
      simulateUncommittedChangedFiles({value: [{path: 'foo.txt', status: 'M'}]});
    });
  });

  it('subscribes to changes to unsaved files', () => {
    expectMessageSentToServer({
      type: 'platform/subscribeToUnsavedFiles',
    });
  });

  it('shows badge with unsaved files', () => {
    act(() =>
      simulateMessageFromServer({
        type: 'platform/unsavedFiles',
        unsaved: [{path: 'foo.txt', uri: 'file:///foo.txt'}],
      }),
    );

    expect(screen.getByText('1 unsaved file')).toBeInTheDocument();

    act(() =>
      simulateMessageFromServer({
        type: 'platform/unsavedFiles',
        unsaved: [
          {path: 'foo.txt', uri: 'file:///foo.txt'},
          {path: 'bar.txt', uri: 'file:///bar.txt'},
        ],
      }),
    );

    expect(screen.getByText('2 unsaved files')).toBeInTheDocument();
  });

  describe('confirms before commit/amend', () => {
    beforeEach(() => {
      act(() =>
        simulateMessageFromServer({
          type: 'platform/unsavedFiles',
          unsaved: [{path: 'foo.txt', uri: 'file:///foo.txt'}],
        }),
      );
    });

    it('allows saving all files', async () => {
      fireEvent.click(screen.getByText('Commit'));
      expect(screen.getByText('You have 1 unsaved file')).toBeInTheDocument();
      fireEvent.click(screen.getByText('Save All and Continue'));
      await waitFor(() => {
        expectMessageSentToServer({
          type: 'platform/saveAllUnsavedFiles',
        });
      });
      act(() => {
        simulateMessageFromServer({
          type: 'platform/savedAllUnsavedFiles',
          success: true,
        });
      });
      await waitFor(() =>
        expectMessageSentToServer({
          type: 'runOperation',
          operation: expect.objectContaining({
            args: expect.arrayContaining(['commit']),
          }),
        }),
      );
    });

    it('allows continuing without saving', async () => {
      fireEvent.click(screen.getByText('Commit'));
      expect(screen.getByText('You have 1 unsaved file')).toBeInTheDocument();
      fireEvent.click(screen.getByText('Continue Without Saving'));
      expectMessageNOTSentToServer({
        type: 'platform/saveAllUnsavedFiles',
      });
      await waitFor(() =>
        expectMessageSentToServer({
          type: 'runOperation',
          operation: expect.objectContaining({
            args: expect.arrayContaining(['commit']),
          }),
        }),
      );
    });

    it('allows cancelling commit', async () => {
      fireEvent.click(screen.getByText('Commit'));
      expect(screen.getByText('You have 1 unsaved file')).toBeInTheDocument();
      fireEvent.click(screen.getByText('Cancel'));
      await nextTick();
      expectMessageNOTSentToServer({
        type: 'platform/saveAllUnsavedFiles',
      });
      expectMessageNOTSentToServer({
        type: 'runOperation',
        operation: expect.anything(),
      });
    });
  });
});
