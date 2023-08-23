/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../../App';
import platform from '../../platform';
import {CommitInfoTestUtils, CommitTreeListTestUtils, ignoreRTL} from '../../testQueries';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  simulateUncommittedChangedFiles,
} from '../../testUtils';
import {CommandRunner} from '../../types';
import {fireEvent, render, screen, within} from '@testing-library/react';
import {act} from 'react-dom/test-utils';
import {nextTick} from 'shared/testUtils';

jest.mock('../../MessageBus');

describe('RevertOperation', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);
    act(() => {
      expectMessageSentToServer({
        type: 'subscribe',
        kind: 'smartlogCommits',
        subscriptionID: expect.anything(),
      });
      simulateCommits({
        value: [
          COMMIT('c', 'Commit C', 'b', {
            filesSample: [{path: 'file.txt', status: 'M'}],
            isHead: true,
          }),
          COMMIT('b', 'Commit B', 'a', {filesSample: [{path: 'file.txt', status: 'M'}]}),
          COMMIT('a', 'Commit A', '1', {filesSample: [{path: 'file.txt', status: 'M'}]}),
          COMMIT('1', 'Commit 1', '0', {phase: 'public'}),
        ],
      });
    });

    // confirm all prompts about reverting files
    jest.spyOn(platform, 'confirm').mockImplementation(() => Promise.resolve(true));
  });

  const clickRevert = async (inside: HTMLElement, fileName: string) => {
    await act(async () => {
      const revertButton = within(
        within(inside).getByTestId(`changed-file-${fileName}`),
      ).getByTestId('file-revert-button');
      expect(revertButton).toBeInTheDocument();
      fireEvent.click(revertButton);
      // confirm modal takes 1 tick to resolve
      await nextTick();
    });
  };

  describe('from uncommitted changes', () => {
    beforeEach(() => {
      act(() => {
        simulateUncommittedChangedFiles({
          value: [
            {path: 'myFile1.txt', status: 'M'},
            {path: 'myFile2.txt', status: 'M'},
          ],
        });
      });
    });

    it('runs revert from uncommitted changes', async () => {
      await clickRevert(screen.getByTestId('commit-tree-root'), 'myFile1.txt');

      expectMessageSentToServer({
        type: 'runOperation',
        operation: {
          args: ['revert', {type: 'repo-relative-file', path: 'myFile1.txt'}],
          id: expect.anything(),
          runner: CommandRunner.Sapling,
          trackEventName: 'RevertOperation',
        },
      });
    });

    it('renders optimistic state while running', async () => {
      expect(
        CommitTreeListTestUtils.withinCommitTree().getByText(ignoreRTL('myFile1.txt')),
      ).toBeInTheDocument();
      await clickRevert(screen.getByTestId('commit-tree-root'), 'myFile1.txt');
      expect(
        CommitTreeListTestUtils.withinCommitTree().queryByText(ignoreRTL('myFile1.txt')),
      ).not.toBeInTheDocument();
    });
  });

  describe('in commit info view for a given commit', () => {
    it('hides revert button on non-head commits', () => {
      CommitInfoTestUtils.clickToSelectCommit('a');

      const revertButton = within(
        within(screen.getByTestId('commit-info-view')).getByTestId(`changed-file-file.txt`),
      ).queryByTestId('file-revert-button');
      expect(revertButton).not.toBeInTheDocument();
    });

    it('reverts before head commit', async () => {
      CommitInfoTestUtils.clickToSelectCommit('c');
      await clickRevert(screen.getByTestId('commit-info-view'), 'file.txt');

      expectMessageSentToServer({
        type: 'runOperation',
        operation: {
          args: [
            'revert',
            '--rev',
            {type: 'succeedable-revset', revset: '.^'},
            {type: 'repo-relative-file', path: 'file.txt'},
          ],
          id: expect.anything(),
          runner: CommandRunner.Sapling,
          trackEventName: 'RevertOperation',
        },
      });
    });

    it('renders optimistic state while running', async () => {
      CommitInfoTestUtils.clickToSelectCommit('c');
      expect(
        CommitTreeListTestUtils.withinCommitTree().queryByText(ignoreRTL('file.txt')),
      ).not.toBeInTheDocument();

      await clickRevert(screen.getByTestId('commit-info-view'), 'file.txt');

      // file is not hidden from the tree, instead it's inserted
      expect(
        CommitTreeListTestUtils.withinCommitTree().getByText(ignoreRTL('file.txt')),
      ).toBeInTheDocument();
    });
  });
});
