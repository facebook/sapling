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
  expectMessageNOTSentToServer,
} from '../../testUtils';
import {CommandRunner} from '../../types';
import {fireEvent, render, screen, within} from '@testing-library/react';
import {act} from 'react-dom/test-utils';
import {nextTick} from 'shared/testUtils';

/* eslint-disable require-await */

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

  const clickDelete = async (inside: HTMLElement, fileName: string) => {
    await act(async () => {
      const revertButton = within(
        within(inside).getByTestId(`changed-file-${fileName}`),
      ).getByTestId('file-action-delete');
      expect(revertButton).toBeInTheDocument();
      fireEvent.click(revertButton);
      // confirm modal takes 1 tick to resolve
      await nextTick();
    });
  };

  const clickCheckboxForFile = async (inside: HTMLElement, fileName: string) => {
    await act(async () => {
      const checkbox = within(within(inside).getByTestId(`changed-file-${fileName}`)).getByTestId(
        'file-selection-checkbox',
      );
      expect(checkbox).toBeInTheDocument();
      fireEvent.click(checkbox);
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

    it('renders optimistic state while running revert', async () => {
      expect(
        CommitTreeListTestUtils.withinCommitTree().getByText(ignoreRTL('myFile1.txt')),
      ).toBeInTheDocument();
      await clickRevert(screen.getByTestId('commit-tree-root'), 'myFile1.txt');
      expect(
        CommitTreeListTestUtils.withinCommitTree().queryByText(ignoreRTL('myFile1.txt')),
      ).not.toBeInTheDocument();
    });

    describe('untracked files get purged', () => {
      beforeEach(() => {
        act(() => {
          simulateUncommittedChangedFiles({
            value: [
              {path: 'myFile1.txt', status: 'M'},
              {path: 'untracked.txt', status: '?'},
            ],
          });
        });
      });

      it('runs purge for untracked uncommitted changes', async () => {
        await clickDelete(screen.getByTestId('commit-tree-root'), 'untracked.txt');

        expectMessageSentToServer({
          type: 'runOperation',
          operation: {
            args: ['purge', '--files', {type: 'repo-relative-file', path: 'untracked.txt'}],
            id: expect.anything(),
            runner: CommandRunner.Sapling,
            trackEventName: 'PurgeOperation',
          },
        });
      });

      it('renders optimistic state while running purge', async () => {
        expect(
          CommitTreeListTestUtils.withinCommitTree().getByText(ignoreRTL('untracked.txt')),
        ).toBeInTheDocument();
        await clickDelete(screen.getByTestId('commit-tree-root'), 'untracked.txt');
        expect(
          CommitTreeListTestUtils.withinCommitTree().queryByText(ignoreRTL('untracked.txt')),
        ).not.toBeInTheDocument();
      });
    });
  });

  describe('bulk discard', () => {
    let confirmSpy: jest.SpyInstance;
    beforeEach(() => {
      confirmSpy = jest.spyOn(platform, 'confirm').mockImplementation(() => Promise.resolve(true));
      act(() => {
        simulateUncommittedChangedFiles({
          value: [
            {path: 'myFile1.txt', status: 'M'},
            {path: 'myFile2.txt', status: 'M'},
            {path: 'untracked1.txt', status: '?'},
            {path: 'untracked2.txt', status: '?'},
          ],
        });
      });
    });

    it('discards all changes with goto --clean if everything selected', async () => {
      await act(async () => {
        fireEvent.click(
          within(screen.getByTestId('commit-tree-root')).getByTestId('discard-all-selected-button'),
        );
      });

      expectMessageSentToServer({
        type: 'runOperation',
        operation: {
          args: ['goto', '--clean', '.'],
          id: expect.anything(),
          runner: CommandRunner.Sapling,
          trackEventName: 'DiscardOperation',
        },
      });

      expectMessageSentToServer({
        type: 'runOperation',
        operation: {
          args: ['purge', '--files'],
          id: expect.anything(),
          runner: CommandRunner.Sapling,
          trackEventName: 'PurgeOperation',
        },
      });

      expect(confirmSpy).toHaveBeenCalled();
    });

    it('discards selected changes with revert and purge', async () => {
      const commitTree = screen.getByTestId('commit-tree-root');
      await clickCheckboxForFile(commitTree, 'myFile1.txt');
      await clickCheckboxForFile(commitTree, 'untracked1.txt');

      await act(async () => {
        fireEvent.click(
          within(screen.getByTestId('commit-tree-root')).getByTestId('discard-all-selected-button'),
        );
      });

      expectMessageSentToServer({
        type: 'runOperation',
        operation: {
          args: ['revert', {type: 'repo-relative-file', path: 'myFile2.txt'}],
          id: expect.anything(),
          runner: CommandRunner.Sapling,
          trackEventName: 'RevertOperation',
        },
      });

      expectMessageSentToServer({
        type: 'runOperation',
        operation: {
          args: ['purge', '--files', {type: 'repo-relative-file', path: 'untracked2.txt'}],
          id: expect.anything(),
          runner: CommandRunner.Sapling,
          trackEventName: 'PurgeOperation',
        },
      });

      expect(confirmSpy).toHaveBeenCalled();
    });

    it('no need to run purge if no files are untracked', async () => {
      const commitTree = screen.getByTestId('commit-tree-root');
      await clickCheckboxForFile(commitTree, 'untracked1.txt');
      await clickCheckboxForFile(commitTree, 'untracked2.txt');

      await act(async () => {
        fireEvent.click(
          within(screen.getByTestId('commit-tree-root')).getByTestId('discard-all-selected-button'),
        );
      });

      expectMessageSentToServer({
        type: 'runOperation',
        operation: {
          args: [
            'revert',
            {type: 'repo-relative-file', path: 'myFile1.txt'},
            {type: 'repo-relative-file', path: 'myFile2.txt'},
          ],
          id: expect.anything(),
          runner: CommandRunner.Sapling,
          trackEventName: 'RevertOperation',
        },
      });

      expectMessageNOTSentToServer({
        type: 'runOperation',
        operation: {
          args: expect.arrayContaining(['purge', '--files']),
          id: expect.anything(),
          runner: CommandRunner.Sapling,
          trackEventName: expect.anything(),
        },
      });

      expect(confirmSpy).toHaveBeenCalled();
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
