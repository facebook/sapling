/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ChangedFile} from '../../types';

import App from '../../App';
import platform from '../../platform';
import {CommitTreeListTestUtils, ignoreRTL} from '../../testQueries';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  closeCommitInfoSidebar,
  simulateMessageFromServer,
} from '../../testUtils';
import {CommandRunner} from '../../types';
import {act, fireEvent, render, screen, waitFor} from '@testing-library/react';
import {wait} from '@testing-library/user-event/dist/utils';

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

    jest.spyOn(platform, 'confirm').mockImplementation(() => Promise.resolve(true));
  });

  const clickUncommit = async (hash: string, filesSample: Array<ChangedFile>) => {
    const quickCommitButton = screen.queryByTestId('uncommit-button');
    act(() => {
      fireEvent.click(quickCommitButton as Element);
    });
    await waitFor(() => {
      expectMessageSentToServer({
        type: 'fetchCommitChangedFiles',
        hash,
        limit: 1000,
      });
    });
    act(() => {
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

    const spy = jest.spyOn(platform, 'confirm').mockImplementation(() => Promise.resolve(true));
    await clickUncommit('c', [FILE1, FILE2, FILE3]);
    expect(spy).toHaveBeenCalledTimes(1);

    expect(withinCommitTree().getByText(ignoreRTL('file1.txt'))).toBeInTheDocument();
    expect(withinCommitTree().getByText(ignoreRTL('file2.txt'))).toBeInTheDocument();
    expect(withinCommitTree().getByText(ignoreRTL('file3.txt'))).toBeInTheDocument();
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
