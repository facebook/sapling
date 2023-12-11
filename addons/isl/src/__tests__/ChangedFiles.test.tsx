/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {UncommittedChanges} from '../types';

import App from '../App';
import {ignoreRTL, CommitInfoTestUtils} from '../testQueries';
import {
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  simulateUncommittedChangedFiles,
  closeCommitInfoSidebar,
  simulateRepoConnected,
  resetTestMessages,
  simulateMessageFromServer,
} from '../testUtils';
import {leftPad} from '../utils';
import {fireEvent, render, screen} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {act} from 'react-dom/test-utils';

jest.mock('../MessageBus');

jest.mock('shared/OperatingSystem', () => ({
  isMac: true,
}));

describe('Changed Files', () => {
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
      expectMessageSentToServer({
        type: 'subscribe',
        kind: 'uncommittedChanges',
        subscriptionID: expect.anything(),
      });
      simulateUncommittedChangedFiles({
        value: [
          {path: 'file1.js', status: 'M'},
          {path: 'src/file2.js', status: 'A'},
          {path: 'src/file3.js', status: 'A'},
          {path: 'src/a/foo.js', status: 'M'},
          {path: 'src/b/foo.js', status: 'M'},
          {path: 'src/subfolder/file4.js', status: 'R'},
          {path: 'src/subfolder/another/yet/another/file5.js', status: 'R'},
        ],
      });
    });
  });

  function openChangedFileTypePicker() {
    const picker = screen.getByTestId('changed-file-display-type-picker');
    expect(picker).toBeInTheDocument();

    act(() => {
      fireEvent.click(picker);
    });
  }

  it('Allows picking changed files display type', () => {
    openChangedFileTypePicker();
    expect(screen.getByText('Short file names')).toBeInTheDocument();
    expect(screen.getByText('Full file paths')).toBeInTheDocument();
    expect(screen.getByText('Tree')).toBeInTheDocument();
    expect(screen.getByText('One-letter directories')).toBeInTheDocument();
  });

  it('Persists choice for display type', () => {
    openChangedFileTypePicker();
    act(() => {
      fireEvent.click(screen.getByText('Tree'));
    });
    expectMessageSentToServer({
      type: 'setConfig',
      name: 'isl.changedFilesDisplayType',
      value: '"tree"',
    });
  });

  it('Updates when config is fetched', () => {
    openChangedFileTypePicker();
    act(() => {
      simulateMessageFromServer({
        type: 'gotConfig',
        name: 'isl.changedFilesDisplayType',
        value: '"fullPaths"',
      });
    });
    expect(screen.getByText(ignoreRTL('src/file2.js'))).toBeInTheDocument();
  });

  it('Uses LTR markers to render paths correctly', () => {
    act(() => {
      simulateUncommittedChangedFiles({
        value: [
          {path: '.gitignore', status: 'M'},
          {path: 'src/.gitignore', status: 'A'},
        ],
      });
    });
    expect(screen.getByText('\u200E.gitignore\u200E')).toBeInTheDocument();
    expect(screen.getByText('\u200Esrc/.gitignore\u200E')).toBeInTheDocument();
  });

  describe('default changed files', () => {
    it('disambiguates file paths', () => {
      expect(screen.getByText(ignoreRTL('file1.js'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('file2.js'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('a/foo.js'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('b/foo.js'))).toBeInTheDocument();

      expect(screen.queryByText(ignoreRTL('src/file2.js'))).not.toBeInTheDocument();
    });
  });

  describe('full file paths', () => {
    it('shows full paths', () => {
      openChangedFileTypePicker();
      act(() => {
        fireEvent.click(screen.getByText('Full file paths'));
      });

      expect(screen.getByText(ignoreRTL('file1.js'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('src/file2.js'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('src/a/foo.js'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('src/b/foo.js'))).toBeInTheDocument();
      expect(
        screen.getByText(ignoreRTL('src/subfolder/another/yet/another/file5.js')),
      ).toBeInTheDocument();
    });

    it('shows full paths when holding alt', () => {
      expect(screen.queryByText(ignoreRTL('src/b/foo.js'))).not.toBeInTheDocument();
      act(() => {
        userEvent.keyboard('{Alt>}'); // '>' means keep pressed
      });
      expect(screen.getByText(ignoreRTL('file1.js'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('src/file2.js'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('src/a/foo.js'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('src/b/foo.js'))).toBeInTheDocument();
      expect(
        screen.getByText(ignoreRTL('src/subfolder/another/yet/another/file5.js')),
      ).toBeInTheDocument();
    });
  });

  describe('one-letter-per-directory file paths', () => {
    it('shows one-letter-per-directory file paths', () => {
      openChangedFileTypePicker();
      act(() => {
        fireEvent.click(screen.getByText('One-letter directories'));
      });

      expect(screen.getByText(ignoreRTL('file1.js'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('s/file2.js'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('s/a/foo.js'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('s/b/foo.js'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('s/s/file4.js'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('s/s/a/y/a/file5.js'))).toBeInTheDocument();
    });
  });

  describe('tree', () => {
    beforeEach(() => {
      openChangedFileTypePicker();
      act(() => {
        fireEvent.click(screen.getByText('Tree'));
      });
    });

    it('shows non-disambiguated file basenames', () => {
      expect(screen.getByText(ignoreRTL('file1.js'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('file2.js'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('file3.js'))).toBeInTheDocument();
      expect(screen.getAllByText(ignoreRTL('foo.js'))).toHaveLength(2);
      expect(screen.getByText(ignoreRTL('file4.js'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('file5.js'))).toBeInTheDocument();
    });

    it('shows folder names', () => {
      expect(screen.getByText(ignoreRTL('src'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('a'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('b'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('subfolder'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('another/yet/another'))).toBeInTheDocument();
    });

    it('clicking folder name hides contents', () => {
      act(() => {
        fireEvent.click(screen.getByText('subfolder'));
      });
      expect(screen.queryByText(ignoreRTL('file4.js'))).not.toBeInTheDocument();
      expect(screen.queryByText(ignoreRTL('file5.js'))).not.toBeInTheDocument();

      expect(screen.getByText(ignoreRTL('file1.js'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('file2.js'))).toBeInTheDocument();
      expect(screen.getByText(ignoreRTL('file3.js'))).toBeInTheDocument();
    });

    it('clicking folders with the same name do not collapse each other', () => {
      act(() => {
        simulateUncommittedChangedFiles({
          value: [
            {path: 'a/foo/file1.js', status: 'M'},
            {path: 'a/file2.js', status: 'M'},
            {path: 'b/foo/file3.js', status: 'M'},
            {path: 'b/file4.js', status: 'M'},
          ],
        });
      });
      act(() => {
        fireEvent.click(screen.getAllByText('foo')[0]);
      });
      expect(screen.queryByText(ignoreRTL('file1.js'))).not.toBeInTheDocument();
      expect(screen.queryByText(ignoreRTL('file3.js'))).toBeInTheDocument();
    });
  });

  describe('truncated list of changed files', () => {
    function makeFiles(n: number): UncommittedChanges {
      return new Array(n)
        .fill(null)
        .map((_, i) => ({path: `file${leftPad(i, 3, '0')}.txt`, status: 'M'}));
    }

    it('only first 500 files are shown', () => {
      act(() => {
        simulateUncommittedChangedFiles({
          value: makeFiles(510),
        });
      });
      const files = screen.getAllByText(/file\d+\.txt/);
      expect(files).toHaveLength(500);
    });

    it('banner is shown if some files are hidden', () => {
      act(() => {
        simulateUncommittedChangedFiles({
          value: makeFiles(700),
        });
      });
      expect(screen.getByText('Showing first 500 files out of 700 total')).toBeInTheDocument();
    });

    it('if more than 500 files are provided, there are page navigation buttons', () => {
      act(() => {
        simulateUncommittedChangedFiles({
          value: makeFiles(510),
        });
      });
      expect(screen.getByTestId('changed-files-next-page')).toBeInTheDocument();
      expect(screen.getByTestId('changed-files-previous-page')).toBeInTheDocument();
      expect(screen.getByTestId('changed-files-previous-page')).toBeDisabled();
      expect(screen.getByText('Showing first 500 files out of 510 total')).toBeInTheDocument();
    });

    it('can click buttons to navigate pages', () => {
      act(() => {
        simulateUncommittedChangedFiles({
          value: makeFiles(1010),
        });
      });
      fireEvent.click(screen.getByTestId('changed-files-next-page'));
      expect(screen.getByText('Showing files 501 – 1000 out of 1010 total')).toBeInTheDocument();
      fireEvent.click(screen.getByTestId('changed-files-next-page'));
      expect(screen.getByText('Showing files 1001 – 1010 out of 1010 total')).toBeInTheDocument();

      expect(screen.getByTestId('changed-files-next-page')).toBeDisabled();

      fireEvent.click(screen.getByTestId('changed-files-previous-page'));
      expect(screen.getByText('Showing files 501 – 1000 out of 1010 total')).toBeInTheDocument();
      fireEvent.click(screen.getByTestId('changed-files-previous-page'));
      expect(screen.getByText('Showing first 500 files out of 1010 total')).toBeInTheDocument();
    });

    it("if more than 500 files exist, but only 500 are provided, don't show pagination buttons", () => {
      act(() => {
        simulateUncommittedChangedFiles({
          value: [],
        });
        simulateCommits({
          value: [
            COMMIT('1', 'some public base', '0', {phase: 'public'}),
            COMMIT('a', 'Commit', '1', {
              isHead: true,
              filesSample: makeFiles(500),
              totalFileCount: 1010,
            }),
          ],
        });
        CommitInfoTestUtils.openCommitInfoSidebar();
      });

      const changedFiles = CommitInfoTestUtils.withinCommitInfo().getByTestId('changed-files');
      expect(changedFiles).toBeInTheDocument();

      // banner shows truncation
      expect(
        CommitInfoTestUtils.withinCommitInfo().getByText(
          'Showing first 500 files out of 1010 total',
        ),
      ).toBeInTheDocument();

      // but no pagination buttons, since we only provide first 25 anyway
      expect(
        CommitInfoTestUtils.withinCommitInfo().queryByTestId('changed-files-next-page'),
      ).not.toBeInTheDocument();
      expect(
        CommitInfoTestUtils.withinCommitInfo().queryByTestId('changed-files-previous-page'),
      ).not.toBeInTheDocument();
    });

    it('if the number of files changes, restrict the page number to fit', () => {
      act(() => {
        simulateUncommittedChangedFiles({
          value: makeFiles(2020),
        });
      });
      fireEvent.click(screen.getByTestId('changed-files-next-page'));
      fireEvent.click(screen.getByTestId('changed-files-next-page'));
      fireEvent.click(screen.getByTestId('changed-files-next-page'));
      fireEvent.click(screen.getByTestId('changed-files-next-page'));
      expect(screen.getByText('Showing files 2001 – 2020 out of 2020 total')).toBeInTheDocument();

      // now some file changes are removed (e.g. discarded)
      act(() => {
        simulateUncommittedChangedFiles({
          value: makeFiles(700),
        });
      });

      // ranges are remapped
      expect(screen.getByText('Showing files 501 – 700 out of 700 total')).toBeInTheDocument();
    });
  });
});
