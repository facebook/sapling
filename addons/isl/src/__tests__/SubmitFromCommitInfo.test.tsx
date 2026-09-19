/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {act, fireEvent, render, screen, waitFor} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import App from '../App';
import {submitReviewersState} from '../CommitInfoView/CommitInfoState';
import {writeAtom} from '../jotaiUtils';
import {CommitInfoTestUtils} from '../testQueries';
import {
  COMMIT,
  expectMessageSentToServer,
  openCommitInfoSidebar,
  resetTestMessages,
  simulateCommits,
  simulateRepoConnected,
  simulateUncommittedChangedFiles,
} from '../testUtils';
import {CommandRunner, exactRevset, succeedableRevset} from '../types';

describe('submitting from Commit Info', () => {
  beforeEach(() => {
    resetTestMessages();
    writeAtom(submitReviewersState('a'), '');
    writeAtom(submitReviewersState('b'), '');
    render(<App />);
    act(() => {
      simulateRepoConnected(undefined, undefined, {preferredSubmitCommand: 'pr'});
      openCommitInfoSidebar();
      simulateUncommittedChangedFiles({value: []});
      simulateCommits({
        value: [
          COMMIT('1', 'Public base', '0', {phase: 'public'}),
          COMMIT('a', 'Selected commit', '1', {totalFileCount: 1}),
          COMMIT('b', 'Head commit', 'a', {isDot: true}),
        ],
      });
    });
  });

  it('defaults to draft submission and shows reviewers below files changed', () => {
    const draftCheckbox = screen.getByRole('checkbox', {name: /Submit as Draft/i});
    expect(draftCheckbox).toBeChecked();

    CommitInfoTestUtils.clickToSelectCommit('a');
    const filesChanged = screen.getByText('Files Changed');
    const reviewers = screen.getByText('Add Reviewer');
    expect(filesChanged.closest('section')?.nextElementSibling).toBe(reviewers.closest('section'));
  });

  it('submits the selected commit and requests entered reviewers', async () => {
    CommitInfoTestUtils.clickToSelectCommit('a');
    await userEvent.type(screen.getByLabelText('Add reviewer'), 'alice,bob');

    fireEvent.click(screen.getByText('Submit Draft'));

    await waitFor(() =>
      expectMessageSentToServer({
        type: 'runOperation',
        operation: {
          args: [
            'pr',
            'submit',
            '--config',
            'github.submit-to-upstream=false',
            '--draft',
            '--rev',
            succeedableRevset('a'),
            '--stack',
            '--reviewer',
            'alice',
            '--reviewer',
            'bob',
          ],
          id: expect.anything(),
          runner: CommandRunner.Sapling,
          trackEventName: 'PrSubmitOperation',
        },
      }),
    );
  });

  it('submits the complete stack', async () => {
    fireEvent.click(screen.getByText('Submit All'));

    await waitFor(() =>
      expectMessageSentToServer({
        type: 'runOperation',
        operation: {
          args: [
            'pr',
            'submit',
            '--config',
            'github.submit-to-upstream=false',
            '--draft',
            '--stack',
          ],
          id: expect.anything(),
          runner: CommandRunner.Sapling,
          trackEventName: 'PrSubmitOperation',
        },
      }),
    );
  });

  it('submits the current commit through a revision resolved at execution time', async () => {
    fireEvent.click(screen.getByText('Submit Draft'));

    await waitFor(() =>
      expectMessageSentToServer({
        type: 'runOperation',
        operation: expect.objectContaining({
          args: [
            'pr',
            'submit',
            '--config',
            'github.submit-to-upstream=false',
            '--draft',
            '--rev',
            exactRevset('.'),
            '--stack',
          ],
        }),
      }),
    );
  });

  it('submits as ready for review when draft submission is disabled', async () => {
    fireEvent.click(screen.getByRole('checkbox', {name: /Submit as Draft/i}));
    fireEvent.click(screen.getByText('Submit All'));

    await waitFor(() =>
      expectMessageSentToServer({
        type: 'runOperation',
        operation: expect.objectContaining({
          args: ['pr', 'submit', '--config', 'github.submit-to-upstream=false', '--stack'],
        }),
      }),
    );
  });
});
