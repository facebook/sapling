/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../../types';

import App from '../../App';
import {Dag, DagCommitInfo} from '../../dag/dag';
import {RebaseOperation} from '../../operations/RebaseOperation';
import {CommitPreview} from '../../previews';
import {ignoreRTL} from '../../testQueries';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  expectMessageNOTSentToServer,
  closeCommitInfoSidebar,
  TEST_COMMIT_HISTORY,
  dragAndDropCommits,
  simulateUncommittedChangedFiles,
  COMMIT,
  scanForkedBranchHashes,
} from '../../testUtils';
import {CommandRunner, succeedableRevset} from '../../types';
import {fireEvent, render, screen, waitFor, within, act} from '@testing-library/react';

/*eslint-disable @typescript-eslint/no-non-null-assertion */

describe('rebase operation', () => {
  // Extend with an obsoleted commit.
  const testHistory = TEST_COMMIT_HISTORY.concat([
    COMMIT('ff1', 'Commit FF1 (obsoleted)', 'z', {successorInfo: {hash: 'ff2', type: 'amend'}}),
    COMMIT('ff2', 'Commit FF2', 'z'),
  ]);

  beforeEach(() => {
    jest.useFakeTimers();
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
        value: testHistory,
      });
    });
  });

  afterEach(() => {
    jest.useRealTimers();
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
    expect(screen.getAllByText('Commit D')).toHaveLength(2);
    // also includes descendants
    expect(screen.getAllByText('Commit E')).toHaveLength(2);

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
    expect(scanForkedBranchHashes('x')).toEqual(['d', 'e']);
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

  it('cannot drag obsoleted commits', () => {
    dragAndDropCommits('ff1', 'e');

    expect(screen.queryByText('Run Rebase')).not.toBeInTheDocument();
    expect(screen.getByText('Cannot rebase obsoleted commits.')).toBeInTheDocument();
  });

  it('can drag if uncommitted changes are optimistically removed', async () => {
    act(() => simulateUncommittedChangedFiles({value: [{path: 'file1.txt', status: 'M'}]}));
    act(() => {
      fireEvent.click(screen.getByTestId('quick-commit-button'));
    });
    await waitFor(() => {
      expect(screen.queryByText(ignoreRTL('file1.txt'))).not.toBeInTheDocument();
    });
    dragAndDropCommits('d', '2');

    expect(
      screen.queryByText('Cannot drag to rebase with uncommitted changes.'),
    ).not.toBeInTheDocument();
  });

  it('can drag with untracked changes', () => {
    act(() => simulateUncommittedChangedFiles({value: [{path: 'file1.txt', status: '?'}]}));
    dragAndDropCommits('d', '2');

    expect(screen.queryByText('Run Rebase')).toBeInTheDocument();
  });

  it('handles partial rebase in optimistic dag', () => {
    const dag = new Dag().add(TEST_COMMIT_HISTORY.map(c => DagCommitInfo.fromCommitInfo(c)));

    const type = 'succeedable-revset';
    // Rebase a-b-c-d-e to z
    const rebaseOp = new RebaseOperation({type, revset: 'a'}, {type, revset: 'z'});
    // Count commits with the given title in a dag.
    const count = (dag: Dag, title: string): number =>
      dag.getBatch([...dag]).filter(c => c.title === title).length;
    // Emulate partial rebased: a-b was rebased to z, but not c-d-e
    const partialRebased = dag.rebase(['a', 'b'], 'z');
    // There are 2 "Commit A"s in the partially rebased dag - one obsolsted.
    expect(count(partialRebased, 'Commit A')).toBe(2);
    expect(count(partialRebased, 'Commit B')).toBe(2);
    expect(partialRebased.descendants('z').size).toBe(dag.descendants('z').size + 2);

    // Calculate the optimistic dag from a partial rebase state.
    const optimisticDag = rebaseOp.optimisticDag(partialRebased);
    // should be only 1 "Commit A"s.
    expect(count(optimisticDag, 'Commit A')).toBe(1);
    expect(count(optimisticDag, 'Commit B')).toBe(1);
    expect(count(optimisticDag, 'Commit E')).toBe(1);
    // check the Commit A..E branch is completed rebased.
    expect(dag.children(dag.parents('a')).size).toBe(
      optimisticDag.children(dag.parents('a')).size + 1,
    );
    expect(optimisticDag.descendants('z').size).toBe(dag.descendants('a').size + 1);
  });

  describe('stacking optimistic state', () => {
    it('cannot drag and drop preview descendants', () => {
      dragAndDropCommits('d', 'a');
      expect(scanForkedBranchHashes('a')).toEqual(['d', 'e']);

      dragAndDropCommits(getCommitWithPreview('e', CommitPreview.REBASE_DESCENDANT), 'b');

      // we still see same commit preview
      expect(scanForkedBranchHashes('a')).toEqual(['d', 'e']);
    });

    it('can drag preview root again', () => {
      dragAndDropCommits('d', 'a');

      dragAndDropCommits(getCommitWithPreview('d', CommitPreview.REBASE_ROOT), 'b');

      // preview is updated to be based on b
      expect(scanForkedBranchHashes('b')).toEqual(['d', 'e']);
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
      expect(scanForkedBranchHashes('a')).toContain('d');
      // also previewing new drag
      expect(scanForkedBranchHashes('b')).toEqual(['e']);
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
      expect(scanForkedBranchHashes('a')).toContain('d');
      // new optimistic state is also there
      expect(scanForkedBranchHashes('b')).toEqual(['e']);
    });
  });
});
