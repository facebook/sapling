/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import {mostRecentSubscriptionIds} from '../serverAPIState';
import {CommitTreeListTestUtils, CommitInfoTestUtils} from '../testQueries';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  simulateRepoConnected,
  TEST_COMMIT_HISTORY,
  COMMIT,
  closeCommitInfoSidebar,
  commitInfoIsOpen,
} from '../testUtils';
import {fireEvent, render, screen} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
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

  const click = (name: string, opts?: {shiftKey?: boolean; metaKey?: boolean}) => {
    act(
      () => void fireEvent.click(CommitTreeListTestUtils.withinCommitTree().getByText(name), opts),
    );
  };

  const expectNoRealSelection = () =>
    expect(CommitInfoTestUtils.withinCommitInfo().queryAllByTestId('selected-commit')).toHaveLength(
      0,
    );

  const expectOnlyOneCommitSelected = () =>
    expect(
      CommitInfoTestUtils.withinCommitInfo().queryByText(/\d Commits Selected/),
    ).not.toBeInTheDocument();

  const expectNCommitsSelected = (n: number) =>
    expect(
      CommitInfoTestUtils.withinCommitInfo().queryByText(`${n} Commits Selected`),
    ).toBeInTheDocument();

  const upArrow = (shift?: boolean) => {
    act(() =>
      userEvent.type(
        screen.getByTestId('commit-tree-root'),
        (shift ? '{shift}' : '') + '{arrowup}',
      ),
    );
  };
  const downArrow = (shift?: boolean) => {
    act(() =>
      userEvent.type(
        screen.getByTestId('commit-tree-root'),
        (shift ? '{shift}' : '') + '{arrowdown}',
      ),
    );
  };

  const rightArrow = () => {
    act(() => userEvent.type(screen.getByTestId('commit-tree-root'), '{arrowright}'));
  };

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

  it("multi selection commit previews doesn't include uncommitted changes", () => {
    act(
      () =>
        void fireEvent.click(CommitTreeListTestUtils.withinCommitTree().getByText('Commit E'), {
          metaKey: true,
        }),
    );
    act(
      () =>
        void fireEvent.click(CommitTreeListTestUtils.withinCommitTree().getByText('Commit D'), {
          metaKey: true,
        }),
    );
    expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit E')).toBeInTheDocument();
    expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit D')).toBeInTheDocument();

    expect(
      CommitInfoTestUtils.withinCommitInfo().queryByText('You are here'),
    ).not.toBeInTheDocument();
    expect(CommitInfoTestUtils.withinCommitInfo().queryByText('Uncommit')).not.toBeInTheDocument();
    expect(CommitInfoTestUtils.withinCommitInfo().queryByText('Go to')).not.toBeInTheDocument();
  });

  describe('shift click selection', () => {
    it('selects ranges of commits when shift-clicking', () => {
      click('Commit B');
      click('Commit D', {shiftKey: true});
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit B')).toBeInTheDocument();
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit C')).toBeInTheDocument();
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit D')).toBeInTheDocument();
      expect(
        CommitInfoTestUtils.withinCommitInfo().getByText('3 Commits Selected'),
      ).toBeInTheDocument();
    });

    it('skips public commits, works across stacks and branches', () => {
      click('Commit D');
      click('Commit Y', {shiftKey: true});
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit D')).toBeInTheDocument();
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit E')).toBeInTheDocument();
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit X')).toBeInTheDocument();
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit Y')).toBeInTheDocument();
      expect(
        CommitInfoTestUtils.withinCommitInfo().getByText('4 Commits Selected'), // skipped '2', the public base of 'Commit X'
      ).toBeInTheDocument();
    });

    it('adds to selection', () => {
      click('Commit A', {metaKey: true});
      click('Commit C', {metaKey: true});
      click('Commit E', {shiftKey: true});
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit A')).toBeInTheDocument();
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit C')).toBeInTheDocument();
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit D')).toBeInTheDocument();
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit E')).toBeInTheDocument();
      expect(
        CommitInfoTestUtils.withinCommitInfo().getByText('4 Commits Selected'),
      ).toBeInTheDocument();
    });

    it('deselecting clears last selected', () => {
      click('Commit A'); // select
      click('Commit A'); // deselect
      click('Commit C', {metaKey: true});
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit C')).toBeInTheDocument();
      // just one commit, C, selected
      expect(
        CommitInfoTestUtils.withinCommitInfo().queryByText(/\d Commits Selected/),
      ).not.toBeInTheDocument();
    });

    it('shift clicking when nothing selected acts like normal clicking', () => {
      click('Commit C', {metaKey: true});
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit C')).toBeInTheDocument();
      expect(
        CommitInfoTestUtils.withinCommitInfo().queryByText(/\d Commits Selected/),
      ).not.toBeInTheDocument();
    });
  });

  describe('up/down arrows to select', () => {
    it('down arrow with no selection starts you at the top', () => {
      expectNoRealSelection();
      downArrow();
      expectOnlyOneCommitSelected();
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit Z')).toBeInTheDocument();
    });

    it('up arrow noop if nothing selected', () => {
      upArrow();
      upArrow(true);
      expectNoRealSelection();
    });

    it('up arrow modifies selection', () => {
      click('Commit C');
      upArrow();
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit D')).toBeInTheDocument();
      expectOnlyOneCommitSelected();
    });

    it('down arrow modifies selection', () => {
      click('Commit C');
      downArrow();
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit B')).toBeInTheDocument();
      expectOnlyOneCommitSelected();
    });

    it('multiple arrow keys keep modifying selection', () => {
      click('Commit A');
      upArrow();
      upArrow();
      upArrow();
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit D')).toBeInTheDocument();
      expectOnlyOneCommitSelected();
    });

    it('selection skips public commits', () => {
      click('Commit A');
      upArrow(); // B
      upArrow(); // C
      upArrow(); // D
      upArrow(); // E
      upArrow(); // skip public base, go to X
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit X')).toBeInTheDocument();
      expectOnlyOneCommitSelected();
    });

    it('goes from last selection if multiple are selected', () => {
      click('Commit A');
      click('Commit C', {metaKey: true});
      upArrow();
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit D')).toBeInTheDocument();
      expectOnlyOneCommitSelected();
    });

    it('holding shift extends upwards', () => {
      click('Commit C');
      upArrow(/* shift */ true);
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit C')).toBeInTheDocument();
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit D')).toBeInTheDocument();
      expectNCommitsSelected(2);
    });

    it('holding shift extends downwards', () => {
      click('Commit C');
      downArrow(/* shift */ true);
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit C')).toBeInTheDocument();
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit B')).toBeInTheDocument();
      expectNCommitsSelected(2);
    });

    it('right arrows opens sidebar', () => {
      click('Commit A');
      act(() => closeCommitInfoSidebar());

      expect(commitInfoIsOpen()).toEqual(false);
      rightArrow();
      expect(CommitInfoTestUtils.withinCommitInfo().getByText('Commit A')).toBeInTheDocument();
      expect(commitInfoIsOpen()).toEqual(true);
    });
  });
});
