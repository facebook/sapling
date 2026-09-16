/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ChangedFile} from '../../types';

import {act, fireEvent, render, screen, waitFor, within} from '@testing-library/react';
import {KeyCode} from 'isl-components/KeyboardShortcuts';
import App from '../../App';
import {__TEST__ as ChangedFilesTestUtils} from '../../ChangedFilesWithFetching';
import {CommitTreeListTestUtils, ignoreRTL} from '../../testQueries';
import {
  COMMIT,
  closeCommitInfoSidebar,
  expectMessageNOTSentToServer,
  expectMessageSentToServer,
  resetTestMessages,
  simulateCommits,
  simulateMessageFromServer,
} from '../../testUtils';
import {CommandRunner} from '../../types';

const {withinCommitTree} = CommitTreeListTestUtils;

const FILEPATH1 = 'file1.txt';
const FILEPATH2 = 'file2.txt';
const FILEPATH3 = 'file3.txt';
const FILE1 = {path: FILEPATH1, status: 'M'} as ChangedFile;
const FILE2 = {path: FILEPATH2, status: 'A'} as ChangedFile;
const FILE3 = {path: FILEPATH3, status: 'R'} as ChangedFile;
describe('UncommitOperation', () => {
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
          COMMIT('1', 'Commit 1', '0', {phase: 'public'}),
          COMMIT('a', 'Commit A', '1', {filePathsSample: [FILEPATH1]}),
          COMMIT('b', 'Commit B', 'a', {filePathsSample: [FILEPATH1, FILEPATH2]}),
          COMMIT('c', 'Commit C', 'b', {
            isDot: true,
            filePathsSample: [FILEPATH1, FILEPATH2, FILEPATH3],
          }),
        ],
      });
    });
  });

  afterEach(() => {
    ChangedFilesTestUtils.commitFilesCache.clear();
  });

  const openUncommitDialog = async (hash: string, filesSample: Array<ChangedFile>) => {
    const quickCommitButton = screen.queryByTestId('uncommit-button');
    act(() => {
      fireEvent.click(quickCommitButton as Element);
    });
    await waitFor(() => {
      expectMessageSentToServer({
        type: 'fetchCommitChangedFiles',
        hash,
        limit: undefined,
      });
    });
    await act(async () => {
      simulateMessageFromServer({
        type: 'fetchedCommitChangedFiles',
        hash,
        result: {
          value: {
            totalFileCount: 3,
            filesSample,
          },
        },
      });
    });
    expectMessageNOTSentToServer({type: 'runOperation', operation: expect.anything()});
    return screen.getByRole('dialog', {name: 'Are you sure you want to Uncommit?'});
  };

  const clickUncommit = async (hash: string, filesSample: Array<ChangedFile>) => {
    const dialog = await openUncommitDialog(hash, filesSample);
    expect(within(dialog).getByRole('button', {name: 'Cancel'})).toHaveFocus();
    fireEvent.click(within(dialog).getByRole('button', {name: 'Uncommit'}));
    await waitFor(() =>
      expectMessageSentToServer({
        type: 'runOperation',
        operation: {
          args: ['uncommit'],
          id: expect.anything(),
          runner: CommandRunner.Sapling,
          trackEventName: 'UncommitOperation',
        },
      }),
    );
  };

  it('confirms before uncommitting', async () => {
    expect(withinCommitTree().queryByText(ignoreRTL('file1.txt'))).not.toBeInTheDocument();
    expect(withinCommitTree().queryByText(ignoreRTL('file2.txt'))).not.toBeInTheDocument();
    expect(withinCommitTree().queryByText(ignoreRTL('file3.txt'))).not.toBeInTheDocument();

    await clickUncommit('c', [FILE1, FILE2, FILE3]);

    expect(withinCommitTree().getByText(ignoreRTL('file1.txt'))).toBeInTheDocument();
    expect(withinCommitTree().getByText(ignoreRTL('file2.txt'))).toBeInTheDocument();
    expect(withinCommitTree().getByText(ignoreRTL('file3.txt'))).toBeInTheDocument();
  });

  it.each(['Cancel', 'Escape'])('does not uncommit when dismissed with %s', async dismissal => {
    const dialog = await openUncommitDialog('c', [FILE1, FILE2, FILE3]);
    await act(async () => {
      if (dismissal === 'Cancel') {
        fireEvent.click(within(dialog).getByRole('button', {name: 'Cancel'}));
      } else {
        fireEvent.keyDown(dialog, {key: 'Escape', keyCode: KeyCode.Escape});
      }
    });
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expectMessageNOTSentToServer({type: 'runOperation', operation: expect.anything()});
    expect(withinCommitTree().queryByText(ignoreRTL('file1.txt'))).not.toBeInTheDocument();
  });

  it('works on commit with children', async () => {
    act(() => {
      simulateCommits({
        value: [
          COMMIT('1', 'Commit 1', '0', {phase: 'public'}),
          COMMIT('a', 'Commit A', '1', {filePathsSample: [FILEPATH1]}),
          COMMIT('b', 'Commit B', 'a', {isDot: true, filePathsSample: [FILEPATH1, FILEPATH2]}),
          COMMIT('c', 'Commit C', 'b', {filePathsSample: [FILEPATH1, FILEPATH2, FILEPATH3]}),
        ],
      });
    });

    expect(withinCommitTree().queryByText(ignoreRTL('file1.txt'))).not.toBeInTheDocument();
    expect(withinCommitTree().queryByText(ignoreRTL('file2.txt'))).not.toBeInTheDocument();
    expect(withinCommitTree().queryByText(ignoreRTL('file3.txt'))).not.toBeInTheDocument();
    await clickUncommit('b', [FILE1, FILE2]);
    expect(withinCommitTree().getByText(ignoreRTL('file1.txt'))).toBeInTheDocument();
    expect(withinCommitTree().getByText(ignoreRTL('file2.txt'))).toBeInTheDocument();
    expect(withinCommitTree().queryByText(ignoreRTL('file3.txt'))).not.toBeInTheDocument();
  });
});
