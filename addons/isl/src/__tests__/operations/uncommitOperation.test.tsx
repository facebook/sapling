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
} from '../../testUtils';
import {CommandRunner} from '../../types';
import {act, fireEvent, render, screen, waitFor} from '@testing-library/react';

const {withinCommitTree} = CommitTreeListTestUtils;

const FILE1 = {path: 'file1.txt', status: 'M'} as ChangedFile;
const FILE2 = {path: 'file2.txt', status: 'A'} as ChangedFile;
const FILE3 = {path: 'file3.txt', status: 'R'} as ChangedFile;
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
          COMMIT('a', 'Commit A', '1', {filesSample: [FILE1]}),
          COMMIT('b', 'Commit B', 'a', {filesSample: [FILE1, FILE2]}),
          COMMIT('c', 'Commit C', 'b', {isDot: true, filesSample: [FILE1, FILE2, FILE3]}),
        ],
      });
    });

    jest.spyOn(platform, 'confirm').mockImplementation(() => Promise.resolve(true));
  });

  const clickUncommit = async () => {
    const quickCommitButton = screen.queryByTestId('uncommit-button');
    act(() => {
      fireEvent.click(quickCommitButton as Element);
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

  it('runs uncommit', async () => {
    await clickUncommit();
  });

  it('confirms before uncommitting', async () => {
    const spy = jest.spyOn(platform, 'confirm').mockImplementation(() => Promise.resolve(true));
    await clickUncommit();
    expect(spy).toHaveBeenCalledTimes(1);
  });

  it('optimistic state works on head commit', async () => {
    expect(withinCommitTree().queryByText(ignoreRTL('file1.txt'))).not.toBeInTheDocument();
    expect(withinCommitTree().queryByText(ignoreRTL('file2.txt'))).not.toBeInTheDocument();
    expect(withinCommitTree().queryByText(ignoreRTL('file3.txt'))).not.toBeInTheDocument();
    await clickUncommit();
    expect(withinCommitTree().getByText(ignoreRTL('file1.txt'))).toBeInTheDocument();
    expect(withinCommitTree().getByText(ignoreRTL('file2.txt'))).toBeInTheDocument();
    expect(withinCommitTree().getByText(ignoreRTL('file3.txt'))).toBeInTheDocument();
  });

  it('works on commit with children', async () => {
    act(() => {
      simulateCommits({
        value: [
          COMMIT('1', 'Commit 1', '0', {phase: 'public'}),
          COMMIT('a', 'Commit A', '1', {filesSample: [FILE1]}),
          COMMIT('b', 'Commit B', 'a', {isDot: true, filesSample: [FILE1, FILE2]}),
          COMMIT('c', 'Commit C', 'b', {filesSample: [FILE1, FILE2, FILE3]}),
        ],
      });
    });

    expect(withinCommitTree().queryByText(ignoreRTL('file1.txt'))).not.toBeInTheDocument();
    expect(withinCommitTree().queryByText(ignoreRTL('file2.txt'))).not.toBeInTheDocument();
    expect(withinCommitTree().queryByText(ignoreRTL('file3.txt'))).not.toBeInTheDocument();
    await clickUncommit();
    expect(withinCommitTree().getByText(ignoreRTL('file1.txt'))).toBeInTheDocument();
    expect(withinCommitTree().getByText(ignoreRTL('file2.txt'))).toBeInTheDocument();
    expect(withinCommitTree().queryByText(ignoreRTL('file3.txt'))).not.toBeInTheDocument();
  });
});
