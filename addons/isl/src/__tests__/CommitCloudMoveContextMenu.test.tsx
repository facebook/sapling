/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {act, fireEvent, render, screen} from '@testing-library/react';
import App from '../App';
import {writeAtom} from '../jotaiUtils';
import {selectedCommits} from '../selection';
import {
  closeCommitInfoSidebar,
  expectMessageSentToServer,
  resetTestMessages,
  simulateCommits,
  simulateMessageFromServer,
  simulateRepoConnected,
  TEST_COMMIT_HISTORY,
} from '../testUtils';
import {succeedableRevset} from '../types';

function seedCloudState(state: {
  currentWorkspace: string;
  workspaceChoices: Array<string>;
  isDisabled?: boolean;
}) {
  act(() => {
    simulateMessageFromServer({
      type: 'fetchedCommitCloudState',
      state: {
        value: {
          lastChecked: new Date(),
          currentWorkspace: state.currentWorkspace,
          workspaceChoices: state.workspaceChoices,
          isDisabled: state.isDisabled,
        },
      },
    });
  });
}

describe('Commit Cloud move-to-workspace context menu', () => {
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
      simulateCommits({value: TEST_COMMIT_HISTORY});
    });
  });

  it('moves a commit to the selected workspace', () => {
    seedCloudState({
      currentWorkspace: 'default',
      workspaceChoices: ['default', 'other-workspace'],
    });

    act(() => {
      fireEvent.contextMenu(screen.getByText('Commit A'));
    });

    const submenu = screen.getByText('Move to workspace');
    act(() => {
      fireEvent.pointerEnter(submenu);
    });

    // Current workspace is excluded from the destination list.
    expect(screen.queryByText('default')).not.toBeInTheDocument();

    act(() => {
      fireEvent.click(screen.getByText('other-workspace'));
    });

    // Revsets — not raw hashes — are dispatched so that queued operations
    // which rewrite these commits get followed to their successor at
    // execution time. The server wraps `succeedable-revset` as
    // `max(successors(<hash>))` on the wire (Repository.ts).
    expectMessageSentToServer({
      type: 'runOperation',
      operation: expect.objectContaining({
        args: ['cloud', 'move', '-d', 'other-workspace', '-r', succeedableRevset('a')],
      }),
    });
  });

  it('hides the menu item when fewer than 2 workspaces exist', () => {
    seedCloudState({
      currentWorkspace: 'default',
      workspaceChoices: ['default'],
    });

    act(() => {
      fireEvent.contextMenu(screen.getByText('Commit A'));
    });

    expect(screen.queryByText('Move to workspace')).not.toBeInTheDocument();
  });

  // Multi-select semantics: when the right-clicked commit is part of an
  // N-commit selection (N > 1), the menu item moves the WHOLE selection
  // in one `sl cloud move` invocation. Pre-fix bug (adversarial-review
  // I3): the handler unconditionally dispatched with `[commit.hash]`,
  // silently dropping the other N-1 selected commits' moves. The
  // adjacent Hide flow in `Commit.tsx::contextMenu()` uses the same
  // `isHideMultiSelect` shape — these tests pin the parallel.
  it('moves all selected commits when the right-clicked commit is part of a multi-selection', () => {
    seedCloudState({
      currentWorkspace: 'default',
      workspaceChoices: ['default', 'other-workspace'],
    });

    // Select three commits — A, B, C — with C being the one we'll
    // right-click. The right-click must operate on all three, not just C.
    act(() => {
      writeAtom(selectedCommits, new Set(['a', 'b', 'c']));
    });

    act(() => {
      fireEvent.contextMenu(screen.getByText('Commit C'));
    });

    // Label reflects the multi-count so the user sees the bulk semantics
    // before clicking. Without this, the only difference from the
    // single-commit case would be invisible until the operation ran.
    const submenu = screen.getByText('Move 3 Commits to workspace');
    act(() => {
      fireEvent.pointerEnter(submenu);
    });

    act(() => {
      fireEvent.click(screen.getByText('other-workspace'));
    });

    expectMessageSentToServer({
      type: 'runOperation',
      operation: expect.objectContaining({
        args: [
          'cloud',
          'move',
          '-d',
          'other-workspace',
          '-r',
          succeedableRevset('a'),
          '-r',
          succeedableRevset('b'),
          '-r',
          succeedableRevset('c'),
        ],
      }),
    });
  });

  it('falls back to single-commit semantics when the right-clicked commit is NOT in the selection', () => {
    seedCloudState({
      currentWorkspace: 'default',
      workspaceChoices: ['default', 'other-workspace'],
    });

    // Select A + B but right-click on C (not in the selection). The
    // menu must act on C alone — the user's right-click target wins
    // over a stale selection of other commits. Mirrors the
    // `isHideMultiSelect.some(c => c.hash === commit.hash)` guard
    // in the sibling Hide flow.
    act(() => {
      writeAtom(selectedCommits, new Set(['a', 'b']));
    });

    act(() => {
      fireEvent.contextMenu(screen.getByText('Commit C'));
    });

    // Label is the single-commit form (no count prefix) — confirms the
    // multi-select branch was NOT taken.
    expect(screen.queryByText('Move 2 Commits to workspace')).not.toBeInTheDocument();
    const submenu = screen.getByText('Move to workspace');
    act(() => {
      fireEvent.pointerEnter(submenu);
    });

    act(() => {
      fireEvent.click(screen.getByText('other-workspace'));
    });

    expectMessageSentToServer({
      type: 'runOperation',
      operation: expect.objectContaining({
        args: ['cloud', 'move', '-d', 'other-workspace', '-r', succeedableRevset('c')],
      }),
    });
  });

  it('single-commit selection collapses to the single-commit code path', () => {
    seedCloudState({
      currentWorkspace: 'default',
      workspaceChoices: ['default', 'other-workspace'],
    });

    // Selection length of 1 must NOT trigger the multi-select branch
    // — the threshold is `length > 1`. Pinning the boundary explicitly
    // catches an off-by-one regression in the gate.
    act(() => {
      writeAtom(selectedCommits, new Set(['a']));
    });

    act(() => {
      fireEvent.contextMenu(screen.getByText('Commit A'));
    });

    expect(screen.queryByText('Move 1 Commits to workspace')).not.toBeInTheDocument();
    expect(screen.getByText('Move to workspace')).toBeInTheDocument();
  });

  it('filters public commits out of the multi-select move (drafts only)', () => {
    seedCloudState({
      currentWorkspace: 'default',
      workspaceChoices: ['default', 'other-workspace'],
    });

    // Adversarial-review I5: when a multi-select includes a mix of
    // draft and public commits and the user right-clicks on a draft,
    // the operation MUST filter out the public ones before dispatching.
    // The outer `!isPublic` gate at the submenu level protects only
    // the right-clicked commit; without this filter the public
    // selection members would slip into the `sl cloud move -r ...`
    // argv and either fail the whole batch server-side or silently
    // skip them (sl version dependent — both bad UX).
    //
    // TEST_COMMIT_HISTORY: '1' and '2' are `phase: 'public'`; 'a' and
    // 'b' are draft. Right-clicking 'a' with selection [a, b, 2]
    // must dispatch only [a, b].
    act(() => {
      writeAtom(selectedCommits, new Set(['a', 'b', '2']));
    });

    act(() => {
      fireEvent.contextMenu(screen.getByText('Commit A'));
    });

    // The label reflects the POST-FILTER count (2, not the selected 3).
    // Using `sourcesToMove.length` for both the label and the dispatch
    // means the user sees a number that matches what `sl cloud move`
    // will actually do — no surprise mismatch between "Move 3 Commits"
    // and an operation that only moves 2.
    const submenu = screen.getByText('Move 2 Commits to workspace');
    act(() => {
      fireEvent.pointerEnter(submenu);
    });

    act(() => {
      fireEvent.click(screen.getByText('other-workspace'));
    });

    // Public commit '2' is filtered out; only drafts 'a' and 'b' dispatch.
    expectMessageSentToServer({
      type: 'runOperation',
      operation: expect.objectContaining({
        args: [
          'cloud',
          'move',
          '-d',
          'other-workspace',
          '-r',
          succeedableRevset('a'),
          '-r',
          succeedableRevset('b'),
        ],
      }),
    });
  });
});
