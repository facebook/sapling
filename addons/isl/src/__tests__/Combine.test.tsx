/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import {CommitInfoTestUtils} from '../testQueries';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  closeCommitInfoSidebar,
  TEST_COMMIT_HISTORY,
  expectMessageNOTSentToServer,
  openCommitInfoSidebar,
} from '../testUtils';
import {CommandRunner} from '../types';
import {fireEvent, render, screen} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {act} from 'react-dom/test-utils';

/*eslint-disable @typescript-eslint/no-non-null-assertion */

jest.mock('../MessageBus');

describe('combine', () => {
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

  describe('shows preview for contiguous commits', () => {
    it('shows preview button for contiguous commits', () => {
      CommitInfoTestUtils.clickToSelectCommit('b', /* cmd click */ true);
      CommitInfoTestUtils.clickToSelectCommit('c', /* cmd click */ true);
      CommitInfoTestUtils.clickToSelectCommit('d', /* cmd click */ true);

      expect(screen.getByText('Combine 3 commits')).toBeInTheDocument();
    });

    it('gaps prevents button', () => {
      CommitInfoTestUtils.clickToSelectCommit('b', /* cmd click */ true);
      CommitInfoTestUtils.clickToSelectCommit('d', /* cmd click */ true);

      expect(screen.queryByText(/Combine \d+ commits/)).not.toBeInTheDocument();
    });
  });

  it('allows cancelling', () => {
    CommitInfoTestUtils.clickToSelectCommit('b', /* cmd click */ true);
    CommitInfoTestUtils.clickToSelectCommit('c', /* cmd click */ true);

    fireEvent.click(screen.getByText('Combine 2 commits'));
    fireEvent.click(screen.getByText('Cancel'));

    expectMessageNOTSentToServer({
      type: 'runOperation',
      operation: expect.anything(),
    });
  });

  it('runs combine operation', () => {
    CommitInfoTestUtils.clickToSelectCommit('b', /* cmd click */ true);
    CommitInfoTestUtils.clickToSelectCommit('c', /* cmd click */ true);
    CommitInfoTestUtils.clickToSelectCommit('d', /* cmd click */ true);

    fireEvent.click(screen.getByText('Combine 3 commits'));
    const runCombineButton = screen.getByText('Run Combine');
    expect(runCombineButton).toBeInTheDocument();
    fireEvent.click(runCombineButton);

    expectMessageSentToServer({
      type: 'runOperation',
      operation: {
        args: [
          'fold',
          '--exact',
          {type: 'exact-revset', revset: 'b::d'},
          '--message',
          'Commit B, Commit C, Commit D\n',
        ],
        id: expect.anything(),
        runner: CommandRunner.Sapling,
        trackEventName: 'FoldOperation',
      },
    });
  });

  it('shows preview of combined message', () => {
    act(() => openCommitInfoSidebar());

    CommitInfoTestUtils.clickToSelectCommit('b', /* cmd click */ true);
    CommitInfoTestUtils.clickToSelectCommit('c', /* cmd click */ true);
    CommitInfoTestUtils.clickToSelectCommit('d', /* cmd click */ true);

    fireEvent.click(CommitInfoTestUtils.withinCommitInfo().getByText('Combine 3 commits'));

    expect(screen.getByText('Previewing result of combined commits')).toBeInTheDocument();

    expect(
      CommitInfoTestUtils.withinCommitInfo().getByText('Commit B, Commit C, Commit D'),
    ).toBeInTheDocument();
  });

  it('allows editing combined message', () => {
    act(() => openCommitInfoSidebar());

    CommitInfoTestUtils.clickToSelectCommit('b', /* cmd click */ true);
    CommitInfoTestUtils.clickToSelectCommit('c', /* cmd click */ true);
    CommitInfoTestUtils.clickToSelectCommit('d', /* cmd click */ true);

    fireEvent.click(CommitInfoTestUtils.withinCommitInfo().getByText('Combine 3 commits'));

    CommitInfoTestUtils.clickToEditDescription();
    act(() => {
      userEvent.type(CommitInfoTestUtils.getDescriptionEditor(), 'new description');
    });

    fireEvent.click(CommitInfoTestUtils.withinCommitInfo().getByText('Run Combine'));

    expectMessageSentToServer({
      type: 'runOperation',
      operation: expect.objectContaining({
        args: [
          'fold',
          '--exact',
          {type: 'exact-revset', revset: 'b::d'},
          '--message',
          expect.stringContaining('new description'),
        ],
      }),
    });
  });

  describe('optimistic state', () => {
    it('shows preview before running', () => {
      CommitInfoTestUtils.clickToSelectCommit('b', /* cmd click */ true);
      CommitInfoTestUtils.clickToSelectCommit('c', /* cmd click */ true);
      CommitInfoTestUtils.clickToSelectCommit('d', /* cmd click */ true);

      fireEvent.click(screen.getByText('Combine 3 commits'));

      act(() => closeCommitInfoSidebar());

      // combined commit is there
      expect(screen.getByText('Commit B, Commit C, Commit D')).toBeInTheDocument();
      // original commits are gone
      expect(screen.queryByText('Commit B')).not.toBeInTheDocument();
      expect(screen.queryByText('Commit C')).not.toBeInTheDocument();
      expect(screen.queryByText('Commit D')).not.toBeInTheDocument();
      // parent and children are still there
      expect(screen.getByText('Commit A')).toBeInTheDocument();
      expect(screen.getByText('Commit E')).toBeInTheDocument();
    });

    it('shows optimistic state when running', () => {
      CommitInfoTestUtils.clickToSelectCommit('b', /* cmd click */ true);
      CommitInfoTestUtils.clickToSelectCommit('c', /* cmd click */ true);
      CommitInfoTestUtils.clickToSelectCommit('d', /* cmd click */ true);

      fireEvent.click(screen.getByText('Combine 3 commits'));
      fireEvent.click(screen.getByText('Run Combine'));

      act(() => closeCommitInfoSidebar());

      // combined commit is there
      expect(screen.getByText('Commit B, Commit C, Commit D')).toBeInTheDocument();
      // original commits are gone
      expect(screen.queryByText('Commit B')).not.toBeInTheDocument();
      expect(screen.queryByText('Commit C')).not.toBeInTheDocument();
      expect(screen.queryByText('Commit D')).not.toBeInTheDocument();
      // parent and children are still there
      expect(screen.getByText('Commit A')).toBeInTheDocument();
      expect(screen.getByText('Commit E')).toBeInTheDocument();
    });
  });
});
