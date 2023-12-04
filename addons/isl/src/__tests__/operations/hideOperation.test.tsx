/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../../App';
import {CommitInfoTestUtils, CommitTreeListTestUtils} from '../../testQueries';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  closeCommitInfoSidebar,
  TEST_COMMIT_HISTORY,
  COMMIT,
} from '../../testUtils';
import {CommandRunner} from '../../types';
import {fireEvent, render, screen, within} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {act} from 'react-dom/test-utils';

/*eslint-disable @typescript-eslint/no-non-null-assertion */

jest.mock('../../MessageBus');

describe('hide operation', () => {
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

  function rightClickAndChooseFromContextMenu(element: Element, choiceMatcher: string) {
    act(() => {
      fireEvent.contextMenu(element);
    });
    const choice = within(screen.getByTestId('context-menu-container')).getByText(choiceMatcher);
    expect(choice).not.toEqual(null);
    act(() => {
      fireEvent.click(choice);
    });
  }

  it('previews hiding a stack of commits', () => {
    rightClickAndChooseFromContextMenu(screen.getByText('Commit B'), 'Hide Commit and Descendants');

    expect(document.querySelectorAll('.commit-preview-hidden-root')).toHaveLength(1);
    expect(document.querySelectorAll('.commit-preview-hidden-descendant')).toHaveLength(3);
  });

  it('runs hide operation', () => {
    rightClickAndChooseFromContextMenu(screen.getByText('Commit B'), 'Hide Commit and Descendants');

    const runHideButton = screen.getByText('Hide');
    expect(runHideButton).toBeInTheDocument();
    fireEvent.click(runHideButton);

    expectMessageSentToServer({
      type: 'runOperation',
      operation: {
        args: ['hide', '--rev', {type: 'succeedable-revset', revset: 'b'}],
        id: expect.anything(),
        runner: CommandRunner.Sapling,
        trackEventName: 'HideOperation',
      },
    });
  });

  it('uses exact revset if commit is obsolete', () => {
    act(() => {
      simulateCommits({
        value: [
          COMMIT('a', 'Commit A', '1', {successorInfo: {hash: 'a2', type: 'rebase'}}),
          COMMIT('a2', 'Commit A2', '2'),
          COMMIT('b', 'Commit B', '1'),
          COMMIT('1', 'some public base', '0', {phase: 'public'}),
          COMMIT('2', 'some public base 2', '1', {phase: 'public'}),
        ],
      });
    });

    rightClickAndChooseFromContextMenu(screen.getByText('Commit A'), 'Hide Commit');

    const runHideButton = screen.getByText('Hide');
    expect(runHideButton).toBeInTheDocument();
    fireEvent.click(runHideButton);

    expectMessageSentToServer({
      type: 'runOperation',
      operation: {
        args: ['hide', '--rev', {type: 'exact-revset', revset: 'a'}],
        id: expect.anything(),
        runner: CommandRunner.Sapling,
        trackEventName: 'HideOperation',
      },
    });
  });

  it('shows optimistic preview of hide', () => {
    rightClickAndChooseFromContextMenu(screen.getByText('Commit B'), 'Hide Commit and Descendants');

    const runHideButton = screen.getByText('Hide');
    fireEvent.click(runHideButton);

    // original commit is hidden
    expect(screen.queryByTestId('commit-b')).not.toBeInTheDocument();
    // same for descendants
    expect(screen.queryByTestId('commit-c')).not.toBeInTheDocument();
    expect(screen.queryByTestId('commit-d')).not.toBeInTheDocument();
    expect(screen.queryByTestId('commit-e')).not.toBeInTheDocument();
  });

  it('does not show uninteresting public base during optimistic hide', () => {
    // Go to another branch so head is not being hidden.
    CommitTreeListTestUtils.clickGoto('z');
    rightClickAndChooseFromContextMenu(screen.getByText('Commit A'), 'Hide Commit and Descendants');

    const runHideButton = screen.getByText('Hide');
    fireEvent.click(runHideButton);

    // the whole subtree is hidden, so the parent commit is not even rendered
    expect(screen.queryByTestId('commit-1')).not.toBeInTheDocument();
  });

  it('shows public base when its the goto preview destination', () => {
    rightClickAndChooseFromContextMenu(screen.getByText('Commit A'), 'Hide Commit and Descendants');

    const runHideButton = screen.getByText('Hide');
    fireEvent.click(runHideButton);

    // the whole subtree and head is hidden, so the parent commit is shown as the goto destination
    expect(screen.queryByTestId('commit-1')).toBeInTheDocument();
  });

  it('does show interesting public base during optimistic hide', () => {
    rightClickAndChooseFromContextMenu(screen.getByText('Commit X'), 'Hide Commit and Descendants');

    const runHideButton = screen.getByText('Hide');
    fireEvent.click(runHideButton);

    // the whole subtree is hidden, but this commit has the remote/master bookmark so it's shown anyway.
    expect(screen.queryByTestId('commit-2')).toBeInTheDocument();
  });

  it('previews a hide by pressing delete with a selection', () => {
    CommitInfoTestUtils.clickToSelectCommit('b');

    act(() => {
      userEvent.type(document.body, '{Backspace}');
    });

    const runHideButton = screen.getByText('Hide');
    expect(runHideButton).toBeInTheDocument();
    expect(runHideButton).toHaveFocus();
    fireEvent.click(runHideButton);

    expectMessageSentToServer({
      type: 'runOperation',
      operation: {
        args: ['hide', '--rev', {type: 'succeedable-revset', revset: 'b'}],
        id: expect.anything(),
        runner: CommandRunner.Sapling,
        trackEventName: 'HideOperation',
      },
    });
  });
});
