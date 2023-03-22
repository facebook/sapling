/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import {mostRecentSubscriptionIds} from '../serverAPIState';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  simulateRepoConnected,
  TEST_COMMIT_HISTORY,
  CommitInfoTestUtils,
  COMMIT,
} from '../testUtils';
import {fireEvent, render, screen} from '@testing-library/react';
import {act} from 'react-dom/test-utils';

jest.mock('../MessageBus');

describe('selection', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);
    act(() => {
      simulateRepoConnected();
      expectMessageSentToServer({
        type: 'subscribe',
        kind: 'smartlogCommits',
        subscriptionID: mostRecentSubscriptionIds.smartlogCommits,
      });
      simulateCommits({value: TEST_COMMIT_HISTORY});
    });
  });

  it('allows selecting via click', () => {
    act(() => void fireEvent.click(screen.getByText('Commit A')));

    expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit A')).toBeInTheDocument();
  });

  it("can't select public commits", () => {
    act(() => void fireEvent.click(screen.getByText('remote/master')));
    // it remains selecting the head commit
    expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit E')).toBeInTheDocument();
  });

  it('click on different commits changes selection', () => {
    act(() => void fireEvent.click(screen.getByText('Commit A')));
    expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit A')).toBeInTheDocument();
    act(() => void fireEvent.click(screen.getByText('Commit B')));
    expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit B')).toBeInTheDocument();

    // not a multi-selection
    expect(
      CommitInfoTestUtils.withinCommitInfo().queryByText(/\d Commits Selected/),
    ).not.toBeInTheDocument();
  });

  it('allows multi-selecting via cmd-click', () => {
    act(() => void fireEvent.click(screen.getByText('Commit A'), {metaKey: true}));
    act(() => void fireEvent.click(screen.getByText('Commit B'), {metaKey: true}));
    act(() => void fireEvent.click(screen.getByText('Commit C'), {metaKey: true}));

    expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit A')).toBeInTheDocument();
    expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit B')).toBeInTheDocument();
    expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit C')).toBeInTheDocument();
    expect(
      CommitInfoTestUtils.withinCommitInfo().getByText('3 Commits Selected'),
    ).toBeInTheDocument();
  });

  it('single click after multi-select resets to single selection', () => {
    act(() => void fireEvent.click(screen.getByText('Commit A'), {metaKey: true}));
    act(() => void fireEvent.click(screen.getByText('Commit B'), {metaKey: true}));

    act(() => void fireEvent.click(screen.getByText('Commit C')));

    expect(CommitInfoTestUtils.withinCommitInfo().queryByText('Commit A')).not.toBeInTheDocument();
    expect(CommitInfoTestUtils.withinCommitInfo().queryByText('Commit B')).not.toBeInTheDocument();
    expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit C')).toBeInTheDocument();

    // not a multi-selection
    expect(
      CommitInfoTestUtils.withinCommitInfo().queryByText(/\d Commits Selected/),
    ).not.toBeInTheDocument();
  });

  it('clicking on a commit a second time deselects it', () => {
    const commitA = screen.getByText('Commit A');
    act(() => void fireEvent.click(commitA));
    expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit A')).toBeInTheDocument();
    act(() => void fireEvent.click(commitA));
    expect(CommitInfoTestUtils.withinCommitInfo().queryByText('Commit A')).not.toBeInTheDocument();
  });

  it('cmd-clicking on a commit a second time deselects it', () => {
    const commitA = screen.getByText('Commit A');
    act(() => void fireEvent.click(commitA, {metaKey: true}));
    act(() => void fireEvent.click(screen.getByText('Commit B'), {metaKey: true}));
    act(() => void fireEvent.click(screen.getByText('Commit C'), {metaKey: true}));

    act(() => void fireEvent.click(commitA, {metaKey: true}));

    expect(CommitInfoTestUtils.withinCommitInfo().queryByText('Commit A')).not.toBeInTheDocument();
    expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit B')).toBeInTheDocument();
    expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit C')).toBeInTheDocument();
    expect(
      CommitInfoTestUtils.withinCommitInfo().getByText('2 Commits Selected'),
    ).toBeInTheDocument();
  });

  it('single click after multi-select resets to single selection, even on a previously selected commits', () => {
    const commitA = screen.getByText('Commit A');
    act(() => void fireEvent.click(commitA, {metaKey: true}));
    act(() => void fireEvent.click(screen.getByText('Commit B'), {metaKey: true}));
    act(() => void fireEvent.click(screen.getByText('Commit C'), {metaKey: true}));

    act(() => void fireEvent.click(commitA));

    expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit A')).toBeInTheDocument();
    expect(CommitInfoTestUtils.withinCommitInfo().queryByText('Commit B')).not.toBeInTheDocument();
    expect(CommitInfoTestUtils.withinCommitInfo().queryByText('Commit C')).not.toBeInTheDocument();

    // not a multi-selection
    expect(
      CommitInfoTestUtils.withinCommitInfo().queryByText(/\d Commits Selected/),
    ).not.toBeInTheDocument();
  });

  it('selecting a commit thats no longer available does not render', () => {
    // add a new commit F, then select it
    act(() => simulateCommits({value: [COMMIT('f', 'Commit F', 'e'), ...TEST_COMMIT_HISTORY]}));
    act(() => void fireEvent.click(screen.getByText('Commit F')));
    // remove that commit from the history
    act(() => simulateCommits({value: TEST_COMMIT_HISTORY}));

    // F no longer exists to show, so now instead the head commit is selected
    expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit E')).toBeInTheDocument();

    // not a multi-selection
    expect(
      CommitInfoTestUtils.withinCommitInfo().queryByText(/\d Commits Selected/),
    ).not.toBeInTheDocument();
  });

  it('does not show the submit button for multi selections in GitHub repos', () => {
    act(() => void fireEvent.click(screen.getByText('Commit A'), {metaKey: true}));
    act(() => void fireEvent.click(screen.getByText('Commit B'), {metaKey: true}));
    expect(
      CommitInfoTestUtils.withinCommitInfo().queryByText('Submit Selected Commits'),
    ).not.toBeInTheDocument();
  });
});
