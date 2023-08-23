/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommandArg} from '../types';

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
} from '../testUtils';
import {fireEvent, render, screen} from '@testing-library/react';
import {act} from 'react-dom/test-utils';

jest.mock('../MessageBus');

describe('CommitTreeList', () => {
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
    });
  });

  it('shows copied files', () => {
    act(() => {
      simulateUncommittedChangedFiles({
        value: [
          {path: 'file_copy.js', status: 'A', copy: 'file.js'},
          {path: 'path/to/file.txt', status: 'A', copy: 'path/original/copiedFrom.txt'},
          {path: 'path/copied/file.txt', status: 'A', copy: 'path/original/file.txt'},
        ],
      });
    });
    expect(screen.getByText(ignoreRTL('file.js → file_copy.js'))).toBeInTheDocument();
    expect(screen.getByText(ignoreRTL('copiedFrom.txt → file.txt'))).toBeInTheDocument();
    expect(screen.getByText(ignoreRTL('original/file.txt → copied/file.txt'))).toBeInTheDocument();
  });

  it("removed files aren't hidden except for renames", () => {
    act(() => {
      simulateUncommittedChangedFiles({
        value: [
          {path: 'file_rem.js', status: 'R'},
          {path: 'file_copy.js', status: 'A', copy: 'file_orig1.js'},
          {path: 'file_rename.js', status: 'A', copy: 'file_orig2.js'},
        ],
      });
    });
    expect(screen.getByText(ignoreRTL('file_rem.js'))).toBeInTheDocument();
  });

  it('shows renamed files', () => {
    act(() => {
      simulateUncommittedChangedFiles({
        value: [
          {path: 'file_rename.js', status: 'A', copy: 'file.js'},
          {path: 'file.js', status: 'R'},
          {path: 'path/to/file.txt', status: 'A', copy: 'path/original/movedFrom.txt'},
          {path: 'path/original/movedFrom.txt', status: 'R'},
          {path: 'path/moved/file.txt', status: 'A', copy: 'path/original/file.txt'},
          {path: 'path/original/file.txt', status: 'R'},
        ],
      });
    });
    expect(screen.getByText(ignoreRTL('file.js → file_rename.js'))).toBeInTheDocument();
    expect(screen.getByText(ignoreRTL('movedFrom.txt → file.txt'))).toBeInTheDocument();
    expect(screen.getByText(ignoreRTL('original/file.txt → moved/file.txt'))).toBeInTheDocument();
    // removed files are visually hidden:
    expect(screen.queryByText(ignoreRTL('file.js'))).not.toBeInTheDocument();
    expect(screen.queryByText(ignoreRTL('movedFrom.txt'))).not.toBeInTheDocument();
    expect(screen.queryByText(ignoreRTL('original/movedFrom.txt'))).not.toBeInTheDocument();
    expect(screen.queryByText(ignoreRTL('path/original/movedFrom.txt'))).not.toBeInTheDocument();
    expect(screen.queryByText(ignoreRTL('moved/file.txt'))).not.toBeInTheDocument();
    expect(screen.queryByText(ignoreRTL('path/moved/file.txt'))).not.toBeInTheDocument();
  });

  it('selecting renamed files selects removed file as well', () => {
    act(() => {
      simulateUncommittedChangedFiles({
        value: [
          {path: 'randomFile.txt', status: 'M'},
          {path: 'path/moved/file.txt', status: 'A', copy: 'path/original/file.txt'},
          {path: 'path/original/file.txt', status: 'R'},
        ],
      });
    });

    // deselect everything
    act(() => {
      fireEvent.click(screen.getByTestId('deselect-all-button'));
    });
    // select the renamed file
    const modifiedFileCheckboxes = document.querySelectorAll(
      '.changed-files .changed-file.file-modified input[type="checkbox"]',
    );
    act(() => {
      fireEvent.click(modifiedFileCheckboxes[1]);
    });
    // create a commit
    act(() => {
      fireEvent.click(screen.getByTestId('quick-commit-button'));
    });

    expectMessageSentToServer({
      type: 'runOperation',
      operation: expect.objectContaining({
        args: [
          'commit',
          '--addremove',
          '--message',
          expect.anything(),
          // both moved and removed file are passed, even though we only selected the single moved file
          {type: 'repo-relative-file', path: 'path/moved/file.txt'},
          {type: 'repo-relative-file', path: 'path/original/file.txt'},
        ] as Array<CommandArg>,
      }),
    });
  });
});
