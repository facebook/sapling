/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../../App';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  simulateUncommittedChangedFiles,
  simulateMessageFromServer,
} from '../../testUtils';
import {CommandRunner} from '../../types';
import {fireEvent, render, screen, waitFor, within} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {act} from 'react-dom/test-utils';

jest.mock('../../MessageBus');

describe('CommitOperation', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);
    act(() => {
      expectMessageSentToServer({
        type: 'subscribe',
        kind: 'smartlogCommits',
        subscriptionID: expect.anything(),
      });
      simulateUncommittedChangedFiles({
        value: [
          {path: 'file1.txt', status: 'M'},
          {path: 'file2.txt', status: 'A'},
          {path: 'file3.txt', status: 'R'},
        ],
      });
      simulateCommits({
        value: [
          COMMIT('2', 'master', '00', {phase: 'public', remoteBookmarks: ['remote/master']}),
          COMMIT('1', 'Commit 1', '0', {phase: 'public'}),
          COMMIT('a', 'Commit A', '1'),
          COMMIT('b', 'Commit B', 'a', {isHead: true}),
        ],
      });
    });
  });

  const clickQuickCommit = () => {
    const quickCommitButton = screen.queryByTestId('quick-commit-button');
    fireEvent.click(quickCommitButton as Element);
  };

  const clickCheckboxForFile = (inside: HTMLElement, fileName: string) => {
    act(() => {
      const checkbox = within(within(inside).getByTestId(`changed-file-${fileName}`)).getByTestId(
        'file-selection-checkbox',
      );
      expect(checkbox).toBeInTheDocument();
      fireEvent.click(checkbox);
    });
  };

  it('runs commit', () => {
    clickQuickCommit();

    expectMessageSentToServer({
      type: 'runOperation',
      operation: {
        args: [
          'commit',
          '--addremove',
          '--message',
          expect.stringContaining(`Temporary Commit at`),
        ],
        id: expect.anything(),
        runner: CommandRunner.Sapling,
        trackEventName: 'CommitOperation',
      },
    });
  });

  it('runs commit with subset of files selected', () => {
    const commitTree = screen.getByTestId('commit-tree-root');
    clickCheckboxForFile(commitTree, 'file2.txt');

    clickQuickCommit();

    expectMessageSentToServer({
      type: 'runOperation',
      operation: {
        args: [
          'commit',
          '--addremove',
          '--message',
          expect.stringContaining(`Temporary Commit at`),
          {type: 'repo-relative-file', path: 'file1.txt'},
          {type: 'repo-relative-file', path: 'file3.txt'},
        ],
        id: expect.anything(),
        runner: CommandRunner.Sapling,
        trackEventName: 'CommitOperation',
      },
    });
  });

  it('changed files are shown in commit info view', () => {
    const commitTree = screen.getByTestId('commit-tree-root');
    clickCheckboxForFile(commitTree, 'file2.txt');

    const quickInput = screen.getByTestId('quick-commit-title');

    act(() => {
      userEvent.type(quickInput, 'My Commit');
    });

    clickQuickCommit();

    expect(
      within(screen.getByTestId('changes-to-amend')).queryByText(/file1.txt/),
    ).not.toBeInTheDocument();
    expect(
      within(screen.getByTestId('changes-to-amend')).getByText(/file2.txt/),
    ).toBeInTheDocument();
    expect(
      within(screen.getByTestId('changes-to-amend')).queryByText(/file3.txt/),
    ).not.toBeInTheDocument();

    expect(
      within(screen.getByTestId('committed-changes')).getByText(/file1.txt/),
    ).toBeInTheDocument();
    expect(
      within(screen.getByTestId('committed-changes')).queryByText(/file2.txt/),
    ).not.toBeInTheDocument();
    expect(
      within(screen.getByTestId('committed-changes')).getByText(/file3.txt/),
    ).toBeInTheDocument();
  });

  it('uses commit template if provided', async () => {
    await waitFor(() => {
      expectMessageSentToServer({type: 'fetchCommitMessageTemplate'});
    });
    act(() => {
      simulateMessageFromServer({
        type: 'fetchedCommitMessageTemplate',
        template: 'Template Title\n\nSummary: my template',
      });
    });

    clickQuickCommit();

    expectMessageSentToServer({
      type: 'runOperation',
      operation: {
        args: ['commit', '--addremove', '--message', expect.stringContaining('Template Title')],
        id: expect.anything(),
        runner: CommandRunner.Sapling,
        trackEventName: 'CommitOperation',
      },
    });
  });
});
