/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash, WorktreeInfo} from './types';

import {atom} from 'jotai';
import serverAPI from './ClientToServerAPI';
import {atomFamilyWeak, readAtom, writeAtom} from './jotaiUtils';
import {WorktreeAddOperation} from './operations/WorktreeAddOperation';
import {WorktreeRemoveOperation} from './operations/WorktreeRemoveOperation';
import {onOperationExited} from './operationsState';
import platform from './platform';
import {registerCleanup, registerDisposable} from './utils';
import {showToast} from './toast';
import {showWorktreeOpenInIDEModal} from './WorktreeIDEModal';

/**
 * Atom holding the list of worktrees for the current repository.
 * Worktrees allow checking out multiple commits simultaneously in separate directories.
 */
export const worktreesAtom = atom<WorktreeInfo[]>([]);

/**
 * Atom family that returns worktrees for a specific commit hash.
 * Matches worktrees where:
 * 1. The current checkout matches the hash exactly, OR
 * 2. The worktree name or path contains the hash prefix (was created for this commit)
 */
export const worktreesForCommit = atomFamilyWeak((hash: Hash) =>
  atom(get => {
    if (!hash) {
      return [];
    }
    const worktrees = get(worktreesAtom);
    const shortHash = hash.slice(0, 8);
    return worktrees.filter(
      wt =>
        wt.commit === hash ||
        wt.commit.startsWith(shortHash) ||
        (wt.name && wt.name.includes(shortHash)) ||
        wt.path.includes(shortHash),
    );
  }),
);

/**
 * Fetches the list of worktrees from the server.
 * Updates the worktreesAtom with the result.
 */
export async function fetchWorktrees(): Promise<WorktreeInfo[]> {
  console.log('[WORKTREE-DEBUG] fetchWorktrees: requesting from server');
  serverAPI.postMessage({type: 'fetchWorktrees'});
  const response = await serverAPI.nextMessageMatching('fetchedWorktrees', () => true);
  if (response.worktrees.error) {
    console.error('[WORKTREE-DEBUG] fetchWorktrees: server returned error', response.worktrees.error);
    throw response.worktrees.error;
  }
  const worktrees = response.worktrees.value;
  console.log('[WORKTREE-DEBUG] fetchWorktrees: received', worktrees.length, 'worktrees', worktrees.map(w => ({name: w.name, path: w.path, commit: w.commit?.slice(0, 8), isMain: w.isMain})));
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
// and optionally open the new worktree in a new VSCode window
registerDisposable(
  worktreesAtom,
  onOperationExited(async (message, operation) => {
    if (operation instanceof WorktreeAddOperation && message.exitCode === 0) {
      console.log('[WORKTREE-DEBUG] WorktreeAddOperation completed successfully');
      const expectedName = operation.getWorktreeName();
      const expectedCommit = operation.getCommit();
      console.log('[WORKTREE-DEBUG] expected worktree:', {expectedName, expectedCommit: expectedCommit.slice(0, 8)});

      // Get worktrees before refresh to compare
      const worktreesBefore = new Set(readAtom(worktreesAtom).map(wt => wt.path));

      try {
        const worktreesAfter = await fetchWorktrees();

        // Find the newly created worktree
        // Match by: not in "before" list AND (name matches OR commit matches OR path contains commit hash)
        const expectedCommitPrefix = expectedCommit.slice(0, 8);
        const newWorktree = worktreesAfter.find(
          wt =>
            !worktreesBefore.has(wt.path) &&
            (wt.name === expectedName ||
              wt.commit.startsWith(expectedCommitPrefix) ||
              wt.path.includes(expectedCommit) ||
              wt.path.includes(expectedCommitPrefix)),
        );

        // Open the new worktree in VSCode
        if (newWorktree) {
          if (platform.platformName === 'vscode') {
            // Direct integration when running inside VSCode
            openWorktreeInVSCode(newWorktree.path);
          } else {
            // Auto-switch ISL cwd to the new worktree
            console.log('[WORKTREE-DEBUG] auto-switching cwd to new worktree:', newWorktree.path);
            serverAPI.postMessage({type: 'changeCwd', cwd: newWorktree.path});
            serverAPI.cwdChanged();
            // Show modal with IDE open command
            showWorktreeOpenInIDEModal(newWorktree.path, newWorktree.name ?? expectedName);
          }
        }
      } catch {
        // Ignore errors when refreshing worktrees
      }
    }
  }),
  import.meta.hot,
);

// Refresh worktrees when a WorktreeRemoveOperation completes
registerDisposable(
  worktreesAtom,
  onOperationExited(async (message, operation) => {
    if (operation instanceof WorktreeRemoveOperation && message.exitCode === 0) {
      try {
        await fetchWorktrees();
        const name = operation.getPath().split(/[/\\]/).pop() ?? operation.getPath();
        showToast(`Worktree "${name}" removed`, {durationMs: 3000});
      } catch {
        // Ignore errors when refreshing worktrees
      }
    }
  }),
  import.meta.hot,
);

/**
 * Open a worktree folder in a new VSCode window (when running inside VSCode).
 */
function openWorktreeInVSCode(worktreePath: string) {
  window.clientToServerAPI?.postMessage({
    type: 'platform/executeVSCodeCommand',
    command: 'vscode.openFolder',
    args: [{scheme: 'file', path: worktreePath}, {forceNewWindow: true}],
  });
}

