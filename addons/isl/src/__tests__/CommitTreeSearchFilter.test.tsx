/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {act, fireEvent, render, screen, waitFor} from '@testing-library/react';
import App from '../App';
import {appliedCommitTreeSearchFilter, commitTreeSearchFilter} from '../CommitTreeSearchFilter';
import {readAtom, writeAtom} from '../jotaiUtils';
import {
  closeCommitInfoSidebar,
  COMMIT,
  resetTestMessages,
  simulateCommits,
  simulateRepoConnected,
} from '../testUtils';

// Comfortably under the debounce window, so a "keystroke" never ends the burst.
const BETWEEN_KEYSTROKES_MS = 50;

describe('commitTreeSearchFilter', () => {
  beforeEach(() => {
    jest.useFakeTimers();
    // Both atoms are module state shared across tests, so put the box back to empty. That
    // also cancels anything a previous test left pending: a debounce can only exist while
    // the box is non-empty, and clearing a non-empty box runs the debouncer's `reset`.
    writeAtom(commitTreeSearchFilter, '');
  });

  afterEach(() => {
    jest.runOnlyPendingTimers();
    jest.useRealTimers();
  });

  it('coalesces a burst of keystrokes into a single applied value', () => {
    for (const typed of ['C', 'Co', 'Com']) {
      act(() => {
        writeAtom(commitTreeSearchFilter, typed);
        jest.advanceTimersByTime(BETWEEN_KEYSTROKES_MS);
      });
      // The box keeps up with the typist...
      expect(readAtom(commitTreeSearchFilter)).toEqual(typed);
      // ...while the value the graph filters by has not moved at all, so nothing
      // that derives from it can have recomputed.
      expect(readAtom(appliedCommitTreeSearchFilter)).toEqual('');
    }

    act(() => {
      jest.runOnlyPendingTimers();
    });
    expect(readAtom(appliedCommitTreeSearchFilter)).toEqual('Com');
  });

  it('applies a cleared filter immediately', () => {
    act(() => {
      writeAtom(commitTreeSearchFilter, 'Commit');
      jest.runOnlyPendingTimers();
    });
    expect(readAtom(appliedCommitTreeSearchFilter)).toEqual('Commit');

    act(() => {
      writeAtom(commitTreeSearchFilter, '');
    });
    expect(readAtom(appliedCommitTreeSearchFilter)).toEqual('');
  });

  it('does not re-apply keystrokes that were pending when the filter was cleared', () => {
    act(() => {
      writeAtom(commitTreeSearchFilter, 'Commit');
      writeAtom(commitTreeSearchFilter, '');
      jest.runOnlyPendingTimers();
    });
    expect(readAtom(appliedCommitTreeSearchFilter)).toEqual('');
  });
});

describe('filtering the commit tree', () => {
  beforeEach(() => {
    resetTestMessages();
    // No filter reset needed here: under `isTest` every `<App />` mount installs a fresh
    // Jotai store, so both atoms start at their defaults.
    render(<App />);
    act(() => {
      simulateRepoConnected();
      closeCommitInfoSidebar();
      simulateCommits({
        value: [
          COMMIT('1', 'some public base', '0', {phase: 'public'}),
          COMMIT('a', 'Commit A', '1'),
          COMMIT('b', 'Commit B', 'a', {isDot: true}),
        ],
      });
    });
  });

  it('keeps typing responsive and re-filters the graph once typing stops', async () => {
    act(() => {
      fireEvent.click(screen.getByTestId('filter-commits-button'));
    });
    const input = screen.getByTestId('commit-tree-search-filter') as HTMLInputElement;

    act(() => {
      fireEvent.input(input, {target: {value: 'Commit A'}});
    });

    // The box shows what was typed right away, and the graph has not been rebuilt yet.
    expect(input.value).toEqual('Commit A');
    expect(screen.getByText('Commit B')).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.queryByText('Commit B')).not.toBeInTheDocument();
    });
    expect(screen.getByText('Commit A')).toBeInTheDocument();
  });
});
