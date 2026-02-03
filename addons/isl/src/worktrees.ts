/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash, WorktreeInfo} from './types';

import {atom} from 'jotai';
import serverAPI from './ClientToServerAPI';
import {atomFamilyWeak, writeAtom} from './jotaiUtils';
import {WorktreeAddOperation} from './operations/WorktreeAddOperation';
import {onOperationExited} from './operationsState';
import {registerCleanup, registerDisposable} from './utils';

/**
 * Atom holding the list of worktrees for the current repository.
 * Worktrees allow checking out multiple commits simultaneously in separate directories.
 */
export const worktreesAtom = atom<WorktreeInfo[]>([]);

/**
 * Atom family that returns worktrees for a specific commit hash.
 * A commit may be checked out in multiple worktrees.
 */
export const worktreesForCommit = atomFamilyWeak((hash: Hash) =>
  atom(get => {
    const worktrees = get(worktreesAtom);
    return worktrees.filter(wt => wt.commit === hash);
  }),
);

/**
 * Fetches the list of worktrees from the server.
 * Updates the worktreesAtom with the result.
 */
export async function fetchWorktrees(): Promise<WorktreeInfo[]> {
  serverAPI.postMessage({type: 'fetchWorktrees'});
  const response = await serverAPI.nextMessageMatching('fetchedWorktrees', () => true);
  if (response.worktrees.error) {
    throw response.worktrees.error;
  }
  const worktrees = response.worktrees.value;
  writeAtom(worktreesAtom, worktrees);
  return worktrees;
}

// Listen for fetched worktrees messages and update the atom
registerDisposable(
  worktreesAtom,
  serverAPI.onMessageOfType('fetchedWorktrees', event => {
    if (event.worktrees.value) {
      writeAtom(worktreesAtom, event.worktrees.value);
    }
  }),
  import.meta.hot,
);

// Fetch worktrees on initial connection
registerCleanup(
  worktreesAtom,
  serverAPI.onSetup(() => {
    fetchWorktrees().catch(() => {
      // Worktree fetching may fail if the wt command is not available
      // (e.g., not a git repo). Silently ignore errors.
    });
  }),
  import.meta.hot,
);

// Refresh worktrees when a WorktreeAddOperation completes successfully
registerDisposable(
  worktreesAtom,
  onOperationExited((message, operation) => {
    if (operation instanceof WorktreeAddOperation && message.exitCode === 0) {
      fetchWorktrees().catch(() => {
        // Ignore errors when refreshing worktrees
      });
    }
  }),
  import.meta.hot,
);
