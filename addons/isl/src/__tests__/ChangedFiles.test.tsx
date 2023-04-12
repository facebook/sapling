/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import {__TEST__} from '../Tooltip';
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
import {fireEvent, render, screen} from '@testing-library/react';
import {act} from 'react-dom/test-utils';

jest.mock('../MessageBus');

describe('Changed Files', () => {
  beforeEach(() => {
    resetTestMessages();
    __TEST__.resetMemoizedTooltipContainer();
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
    expect(screen.getByText('src/file2.js')).toBeInTheDocument();
  });

  describe('default changed files', () => {
    it('disambiguates file paths', () => {
      expect(screen.getByText('file1.js')).toBeInTheDocument();
      expect(screen.getByText('file2.js')).toBeInTheDocument();
      expect(screen.getByText('a/foo.js')).toBeInTheDocument();
      expect(screen.getByText('b/foo.js')).toBeInTheDocument();

      expect(screen.queryByText('src/file2.js')).not.toBeInTheDocument();
    });
  });

  describe('full file paths', () => {
    it('shows full paths', () => {
      openChangedFileTypePicker();
      act(() => {
        fireEvent.click(screen.getByText('Full file paths'));
      });

      expect(screen.getByText('file1.js')).toBeInTheDocument();
      expect(screen.getByText('src/file2.js')).toBeInTheDocument();
      expect(screen.getByText('src/a/foo.js')).toBeInTheDocument();
      expect(screen.getByText('src/b/foo.js')).toBeInTheDocument();
      expect(screen.getByText('src/subfolder/another/yet/another/file5.js')).toBeInTheDocument();
    });
  });

  describe('one-letter-per-directory file paths', () => {
    it('shows one-letter-per-directory file paths', () => {
      openChangedFileTypePicker();
      act(() => {
        fireEvent.click(screen.getByText('One-letter directories'));
      });

      expect(screen.getByText('file1.js')).toBeInTheDocument();
      expect(screen.getByText('s/file2.js')).toBeInTheDocument();
      expect(screen.getByText('s/a/foo.js')).toBeInTheDocument();
      expect(screen.getByText('s/b/foo.js')).toBeInTheDocument();
      expect(screen.getByText('s/s/file4.js')).toBeInTheDocument();
      expect(screen.getByText('s/s/a/y/a/file5.js')).toBeInTheDocument();
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
      expect(screen.getByText('file1.js')).toBeInTheDocument();
      expect(screen.getByText('file2.js')).toBeInTheDocument();
      expect(screen.getByText('file3.js')).toBeInTheDocument();
      expect(screen.getAllByText('foo.js')).toHaveLength(2);
      expect(screen.getByText('file4.js')).toBeInTheDocument();
      expect(screen.getByText('file5.js')).toBeInTheDocument();
    });

    it('shows folder names', () => {
      expect(screen.getByText('src')).toBeInTheDocument();
      expect(screen.getByText('a')).toBeInTheDocument();
      expect(screen.getByText('b')).toBeInTheDocument();
      expect(screen.getByText('subfolder')).toBeInTheDocument();
      expect(screen.getByText('another/yet/another')).toBeInTheDocument();
    });

    it('clicking folder name hides contents', () => {
      act(() => {
        fireEvent.click(screen.getByText('subfolder'));
      });
      expect(screen.queryByText('file4.js')).not.toBeInTheDocument();
      expect(screen.queryByText('file5.js')).not.toBeInTheDocument();

      expect(screen.getByText('file1.js')).toBeInTheDocument();
      expect(screen.getByText('file2.js')).toBeInTheDocument();
      expect(screen.getByText('file3.js')).toBeInTheDocument();
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
      expect(screen.queryByText('file1.js')).not.toBeInTheDocument();
      expect(screen.queryByText('file3.js')).toBeInTheDocument();
    });
  });
});
