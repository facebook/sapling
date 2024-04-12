/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import {CommitInfoTestUtils} from '../testQueries';
import {
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  openCommitInfoSidebar,
  simulateMessageFromServer,
} from '../testUtils';
import {fireEvent, render, screen, waitFor, within, act} from '@testing-library/react';
import userEvent from '@testing-library/user-event';

/* eslint-disable @typescript-eslint/no-non-null-assertion */

const {
  withinCommitInfo,
  clickAmendMode,
  clickCommitMode,
  clickToSelectCommit,
  getTitleEditor,
  getDescriptionEditor,
} = CommitInfoTestUtils;

describe('FillCommitMessage', () => {
  beforeEach(() => {
    render(<App />);
    act(() => {
      openCommitInfoSidebar();
      expectMessageSentToServer({
        type: 'subscribe',
        kind: 'smartlogCommits',
        subscriptionID: expect.anything(),
      });
      simulateCommits({
        value: [
          COMMIT('1', 'some public base', '0', {phase: 'public'}),
          COMMIT('a', 'My Commit', '1'),
          COMMIT('b', 'Head Commit', 'a', {
            isDot: true,
            description: 'Summary: This is my commit message\n',
          }),
        ],
      });
    });
  });

  it('Shows fill message buttons in commit mode', () => {
    clickCommitMode();
    expect(screen.getByText('Fill commit message from', {exact: false})).toBeInTheDocument();
  });

  it('does not show fill buttons in amend mode', () => {
    clickCommitMode();
    clickToSelectCommit('a');
    expect(screen.queryByText('Fill commit message from', {exact: false})).not.toBeInTheDocument();
    clickToSelectCommit('b');
    clickAmendMode();
    expect(screen.queryByText('Fill commit message from', {exact: false})).not.toBeInTheDocument();
  });

  it('Load from last commit', async () => {
    clickCommitMode();

    expect(getTitleEditor()).toHaveValue('');
    expect(getDescriptionEditor()).toHaveValue('');

    const loadFromLastCommit = withinCommitInfo().getByText('last commit');
    expect(loadFromLastCommit).toBeInTheDocument();
    fireEvent.click(loadFromLastCommit);
    await waitFor(() => {
      expect(getTitleEditor().value).toMatch('Head Commit');
      expect(getDescriptionEditor().value).toMatch(/This is my commit message/);
    });
  });

  it('Load from commit template', async () => {
    expectMessageSentToServer({type: 'fetchCommitMessageTemplate'});
    act(() => {
      simulateMessageFromServer({
        type: 'fetchedCommitMessageTemplate',
        template: 'template title\nSummary: template summary',
      });
    });
    clickCommitMode();

    // the template is used automatically, so let's clear it out
    act(() => {
      userEvent.clear(getTitleEditor());
      userEvent.clear(getDescriptionEditor());
    });

    expect(getTitleEditor()).toHaveValue('');
    expect(getDescriptionEditor()).toHaveValue('');

    const loadFromLastCommit = withinCommitInfo().getByText('template file');
    expect(loadFromLastCommit).toBeInTheDocument();
    fireEvent.click(loadFromLastCommit);
    await waitFor(() => {
      expect(getTitleEditor().value).toMatch('template title');
      expect(getDescriptionEditor().value).toMatch(/template summary/);
    });
  });

  describe('conflicts in message to fill', () => {
    async function triggerConflict() {
      clickCommitMode();

      act(() => {
        userEvent.type(getTitleEditor(), 'existing title');
        userEvent.type(getDescriptionEditor(), 'Summary: existing description');
      });

      const loadFromLastCommit = withinCommitInfo().getByText('last commit');
      expect(loadFromLastCommit).toBeInTheDocument();
      fireEvent.click(loadFromLastCommit);

      await waitFor(() => {
        expect(screen.getByText('Commit Messages Conflict')).toBeInTheDocument();
      });
    }

    function withinConflictWarning() {
      return within(screen.getByTestId('fill-message-conflict-warning'));
    }

    it('shows warning and differences', async () => {
      await triggerConflict();

      expect(withinConflictWarning().getByText('Title')).toBeInTheDocument();
      expect(withinConflictWarning().getByText('existing title')).toBeInTheDocument();
      expect(withinConflictWarning().getByText('Head Commit')).toBeInTheDocument();
      expect(
        withinConflictWarning().getByText('existing description', {exact: false}),
      ).toBeInTheDocument();
      expect(
        withinConflictWarning().getByText('This is my commit message', {exact: false}),
      ).toBeInTheDocument();
    });

    it('allows merging', async () => {
      await triggerConflict();
      const mergeButton = screen.getByText('Merge');
      expect(mergeButton).toBeInTheDocument();
      fireEvent.click(mergeButton);

      await waitFor(() => {
        expect(getTitleEditor().value).toMatch('existing title, Head Commit');
        expect(getDescriptionEditor().value).toMatch(/existing description/);
        expect(getDescriptionEditor().value).toMatch(/This is my commit message/);
      });
    });

    it('allows cancelling', async () => {
      await triggerConflict();
      const cancelButton = screen.getByText('Cancel');
      expect(cancelButton).toBeInTheDocument();
      fireEvent.click(cancelButton);

      expect(screen.queryByText('Commit Messages Conflict')).not.toBeInTheDocument();
      await waitFor(() => {
        expect(getTitleEditor().value).toMatch('existing title');
        expect(getDescriptionEditor().value).toMatch(/existing description/);
      });
    });

    it('allows overwriting', async () => {
      await triggerConflict();
      const overwrite = screen.getByText('Overwrite');
      expect(overwrite).toBeInTheDocument();
      fireEvent.click(overwrite);

      await waitFor(() => {
        expect(getTitleEditor().value).toMatch('Head Commit');
        expect(getDescriptionEditor().value).toMatch(/This is my commit message/);
      });
    });
  });
});
