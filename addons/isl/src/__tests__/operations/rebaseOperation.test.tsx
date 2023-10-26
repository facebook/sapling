/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../../types';

import App from '../../App';
import {CommitPreview} from '../../previews';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  expectMessageNOTSentToServer,
  closeCommitInfoSidebar,
  TEST_COMMIT_HISTORY,
  dragAndDropCommits,
  simulateUncommittedChangedFiles,
} from '../../testUtils';
import {CommandRunner, succeedableRevset} from '../../types';
import {fireEvent, render, screen, within} from '@testing-library/react';
import {act} from 'react-dom/test-utils';

/*eslint-disable @typescript-eslint/no-non-null-assertion */

jest.mock('../../MessageBus');

describe('rebase operation', () => {
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
        value: TEST_COMMIT_HISTORY,
      });
    });
  });

  const getCommitWithPreview = (hash: Hash, preview: CommitPreview): HTMLElement => {
    const previewOfCommit = screen
      .queryAllByTestId(`commit-${hash}`)
      .map(commit => within(commit).queryByTestId('draggable-commit'))
      .find(commit => commit?.classList.contains(`commit-preview-${preview}`));
    expect(previewOfCommit).toBeInTheDocument();
    return previewOfCommit!;
  };

  it('previews a rebase on drag & drop onto a public commit', () => {
    expect(screen.getAllByText('Commit D')).toHaveLength(1);
    dragAndDropCommits('d', '2');

    // original commit AND previewed commit are now in the document
    expect(screen.queryAllByTestId('commit-d')).toHaveLength(2);
    // also includes descendants
    expect(screen.queryAllByTestId('commit-e')).toHaveLength(2);

    // one of them is a rebase preview
    expect(
      screen
        .queryAllByTestId('commit-d')
        .some(commit => commit.querySelector('.commit-preview-rebase-root')),
    ).toEqual(true);
  });

  it('sets all descendants as the right preview type', () => {
    expect(screen.getAllByText('Commit D')).toHaveLength(1);
    dragAndDropCommits('a', '2');

    expect(document.querySelectorAll('.commit-preview-rebase-old')).toHaveLength(5);
    expect(document.querySelectorAll('.commit-preview-rebase-root')).toHaveLength(1);
    expect(document.querySelectorAll('.commit-preview-rebase-descendant')).toHaveLength(4);
  });

  it('previews onto correct branch', () => {
    expect(screen.getAllByText('Commit D')).toHaveLength(1);
    dragAndDropCommits('d', 'x');

    expect(within(screen.getByTestId('branch-from-x')).queryByTestId('commit-d'));
    expect(within(screen.getByTestId('branch-from-x')).queryByTestId('commit-e'));
  });

  it('cannot drag public commits', () => {
    dragAndDropCommits('1', '2');

    // only see one copy of commit 1
    expect(screen.queryAllByTestId('commit-1')).toHaveLength(1);
  });

  it('runs rebase operation', () => {
    dragAndDropCommits('d', '2');

    const runRebaseButton = screen.getByText('Run Rebase');
    expect(runRebaseButton).toBeInTheDocument();

    fireEvent.click(runRebaseButton);

    expectMessageSentToServer({
      type: 'runOperation',
      operation: {
        args: ['rebase', '-s', succeedableRevset('d'), '-d', succeedableRevset('remote/master')],
        id: expect.anything(),
        runner: CommandRunner.Sapling,
        trackEventName: 'RebaseOperation',
      },
    });
  });

  it('shows optimistic preview of rebase', () => {
    dragAndDropCommits('d', '2');

    fireEvent.click(screen.getByText('Run Rebase'));

    // original commit is hidden, we only see optimistic commit
    expect(screen.queryAllByTestId('commit-d')).toHaveLength(1);
    // also includes descendants
    expect(screen.queryAllByTestId('commit-e')).toHaveLength(1);

    expect(screen.getByText('rebasing...')).toBeInTheDocument();

    expect(
      screen.queryByTestId('commit-d')?.querySelector('.commit-preview-rebase-optimistic-root'),
    ).toBeInTheDocument();
  });

  it('cancel cancels the preview', () => {
    dragAndDropCommits('d', '2');

    const cancelButton = screen.getByText('Cancel');
    expect(cancelButton).toBeInTheDocument();

    act(() => {
      fireEvent.click(cancelButton);
    });

    // now the preview doesn't exist
    expect(screen.queryAllByTestId('commit-d')).toHaveLength(1);

    // we didn't run any operation
    expectMessageNOTSentToServer({
      type: 'runOperation',
      operation: expect.anything(),
    });
  });

  it('cannot drag with uncommitted changes', () => {
    act(() => simulateUncommittedChangedFiles({value: [{path: 'file1.txt', status: 'M'}]}));
    dragAndDropCommits('d', '2');

    expect(screen.queryByText('Run Rebase')).not.toBeInTheDocument();
    expect(screen.getByText('Cannot drag to rebase with uncommitted changes.')).toBeInTheDocument();
  });

  it('can drag with untracked changes', () => {
    act(() => simulateUncommittedChangedFiles({value: [{path: 'file1.txt', status: '?'}]}));
    dragAndDropCommits('d', '2');

    expect(screen.queryByText('Run Rebase')).toBeInTheDocument();
  });

  describe('stacking optimistic state', () => {
    it('cannot drag and drop preview descendants', () => {
      dragAndDropCommits('d', 'a');

      expect(
        within(screen.getByTestId('branch-from-a')).queryByTestId('commit-d'),
      ).toBeInTheDocument();
      expect(
        within(screen.getByTestId('branch-from-a')).queryByTestId('commit-e'),
      ).toBeInTheDocument();

      dragAndDropCommits(getCommitWithPreview('e', CommitPreview.REBASE_DESCENDANT), 'b');

      // we still see same commit preview
      expect(
        within(screen.getByTestId('branch-from-a')).queryByTestId('commit-d'),
      ).toBeInTheDocument();
      expect(
        within(screen.getByTestId('branch-from-a')).queryByTestId('commit-e'),
      ).toBeInTheDocument();
    });

    it('can drag preview root again', () => {
      dragAndDropCommits('d', 'a');

      dragAndDropCommits(getCommitWithPreview('d', CommitPreview.REBASE_ROOT), 'b');

      // preview is updated to be based on b
      expect(
        within(screen.getByTestId('branch-from-b')).queryByTestId('commit-d'),
      ).toBeInTheDocument();
      expect(
        within(screen.getByTestId('branch-from-b')).queryByTestId('commit-e'),
      ).toBeInTheDocument();
    });

    it('can preview drag drop while previous rebase running', () => {
      //              c
      //       c      | e
      // e     b      |/
      // d     | e    b
      // c  -> | d -> | d
      // b     |/     |/
      // a     a      a
      dragAndDropCommits('d', 'a');
      fireEvent.click(screen.getByText('Run Rebase'));

      dragAndDropCommits(
        getCommitWithPreview('e', CommitPreview.REBASE_OPTIMISTIC_DESCENDANT),
        'b',
      );

      // original optimistic is still there
      expect(
        within(screen.getByTestId('branch-from-a')).queryByTestId('commit-d'),
      ).toBeInTheDocument();
      // also previewing new drag
      expect(
        within(screen.getByTestId('branch-from-b')).queryByTestId('commit-e'),
      ).toBeInTheDocument();
    });

    it('can see optimistic drag drop while previous rebase running', () => {
      //              c
      //       c      | e
      // e     b      |/
      // d     | e    b
      // c  -> | d -> | d
      // b     |/     |/
      // a     a      a
      dragAndDropCommits('d', 'a');
      fireEvent.click(screen.getByText('Run Rebase'));

      dragAndDropCommits(
        getCommitWithPreview('e', CommitPreview.REBASE_OPTIMISTIC_DESCENDANT),
        'b',
      );
      fireEvent.click(screen.getByText('Run Rebase'));

      // original optimistic is still there
      expect(
        within(screen.getByTestId('branch-from-a')).queryByTestId('commit-d'),
      ).toBeInTheDocument();
      // new optimistic state is also there
      expect(
        within(screen.getByTestId('branch-from-b')).queryByTestId('commit-e'),
      ).toBeInTheDocument();
    });
  });
});
