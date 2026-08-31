/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash, WorktreeEntry} from './types';

import {atom} from 'jotai';
import {basename, guessPathSep, isHexHash, pathsAreIdentical} from 'shared/utils';
import {focusMode} from './atoms/FocusModeState';
import {featureFlagLoadable} from './featureFlags';
import {Internal} from './Internal';
import {atomFamilyWeak, localStorageBackedAtom} from './jotaiUtils';
import {repositoryInfo, worktreeInfoData} from './serverAPIState';

/** Whether to show worktree name labels ("You are here" label, checked-out-elsewhere badges). */
export const showWorktreeLabels = localStorageBackedAtom<boolean>('isl.show-worktree-labels', true);

/**
 * `{info, worktreeInfo}` when the worktrees feature is enabled, this is an EdenFS
 * repo, and worktree data has loaded -- `undefined` otherwise. Shared gating logic
 * for the atoms below, so non-EdenFs/flag-off repos never pay for the extra reads.
 */
const enabledWorktreeInfo = atom(get => {
  const info = get(repositoryInfo);
  if (info?.isEdenFs !== true) {
    return undefined;
  }

  const flag = get(featureFlagLoadable(Internal.featureFlags?.Worktrees));
  if (flag.state !== 'hasData' || flag.data !== true) {
    return undefined;
  }

  const worktreeInfo = get(worktreeInfoData);
  if (worktreeInfo == null) {
    return undefined;
  }

  return {info, worktreeInfo};
});

/** All sibling-worktree checkout locations, independent of whether labels are visible. */
export const allOtherWorktreeCheckoutsByHash = atom(get => {
  const empty = new Map<Hash, Array<WorktreeEntry>>();

  const enabled = get(enabledWorktreeInfo);
  if (enabled == null) {
    return empty;
  }
  const {info, worktreeInfo} = enabled;

  const map = new Map<Hash, Array<WorktreeEntry>>();
  for (const worktree of worktreeInfo.worktrees) {
    if (worktree.node == null || pathsAreIdentical(worktree.path, info.repoRoot)) {
      continue;
    }
    const trimmed = worktree.node.trim();
    if (trimmed === '' || !isHexHash(trimmed)) {
      continue;
    }
    const key = trimmed as Hash;
    // Store normalized trimmed node so key and entry stay in sync
    const normalized: WorktreeEntry = {...worktree, node: trimmed as Hash};
    const existing = map.get(key);
    if (existing != null) {
      existing.push(normalized);
    } else {
      map.set(key, [normalized]);
    }
  }
  return map;
});

/**
 * Sibling-worktree checkout locations that should be rendered as labels. Focus
 * mode and the display preference suppress the labels without discarding the
 * underlying checkout identity used by focus-mode filtering.
 */
export const otherWorktreeCheckoutsByHash = atom(get => {
  if (!get(showWorktreeLabels) || get(focusMode)) {
    return new Map<Hash, Array<WorktreeEntry>>();
  }
  return get(allOtherWorktreeCheckoutsByHash);
});

/**
 * The sibling worktree(s) currently checked out at this commit, if any.
 * Analogous to `isHighlightedCommit` in `HighlightedCommits.tsx`.
 */
export const isCheckedOutElsewhere = atomFamilyWeak((hash: Hash) =>
  atom(get => get(otherWorktreeCheckoutsByHash).get(hash)),
);

/**
 * This repo's own worktree name (label, or basename of its path), but only when
 * this checkout *is* a linked worktree (not the main one) -- `undefined` for a
 * solo checkout or the main worktree, so "You are here" stays plain there.
 */
export const currentWorktreeName = atom(get => {
  if (!get(showWorktreeLabels)) {
    return undefined;
  }
  const enabled = get(enabledWorktreeInfo);
  if (enabled == null || enabled.worktreeInfo.worktrees.length <= 1) {
    return undefined;
  }
  const {info, worktreeInfo} = enabled;
  const mine = worktreeInfo.worktrees.find(wt => pathsAreIdentical(wt.path, info.repoRoot));
  if (mine == null || mine.role !== 'linked') {
    return undefined;
  }
  return mine.label ?? basename(mine.path, guessPathSep(mine.path));
});
