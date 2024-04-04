/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ISLCommandName} from './ISLShortcuts';
import type {CommitInfo, Hash} from './types';
import type React from 'react';

import {commitMode} from './CommitInfoView/CommitInfoState';
import {useCommand} from './ISLShortcuts';
import {useSelectAllCommitsShortcut} from './SelectAllCommits';
import {latestSuccessorUnlessExplicitlyObsolete, successionTracker} from './SuccessionTracker';
import {YOU_ARE_HERE_VIRTUAL_COMMIT} from './dag/virtualCommit';
import {islDrawerState} from './drawerState';
import {readAtom, useAtomHas, writeAtom} from './jotaiUtils';
import {HideOperation} from './operations/HideOperation';
import {operationBeingPreviewed} from './operationsState';
import {dagWithPreviews} from './previews';
import {latestDag} from './serverAPIState';
import {firstOfIterable, registerCleanup} from './utils';
import {atom} from 'jotai';
import {useCallback} from 'react';
import {isMac} from 'shared/OperatingSystem';

/**
 * The name of the key to toggle individual selection.
 * On Windows / Linux, it is Ctrl. On Mac, it is Command.
 */
export const individualToggleKey: 'metaKey' | 'ctrlKey' = isMac ? 'metaKey' : 'ctrlKey';

/**
 * See {@link selectedCommitInfos}
 * Note: it is possible to be selecting a commit that stops being rendered, and thus has no associated commit info.
 * Prefer to use `selectedCommitInfos` to get the subset of the selection that is visible.
 */
export const selectedCommits = atom(new Set<Hash>());
registerCleanup(
  selectedCommits,
  successionTracker.onSuccessions(successions => {
    let value = readAtom(selectedCommits);
    let changed = false;

    for (const [oldHash, newHash] of successions) {
      if (value?.has(oldHash)) {
        if (!changed) {
          changed = true;
          value = new Set(value);
        }
        value.delete(oldHash);
        value.add(newHash);
      }
    }
    if (changed) {
      writeAtom(selectedCommits, value);
    }
  }),
  import.meta.hot,
);

const previouslySelectedCommit = atom<undefined | string>(undefined);

/**
 * Clicking on commits will select them in the UI.
 * Selected commits can be acted on in bulk, and appear in the commit info sidebar for editing / details.
 * Invariant: Selected commits are non-public.
 *
 * See {@link selectedCommits} for setting underlying storage
 */
export const selectedCommitInfos = atom(get => {
  const selected = get(selectedCommits);
  const dag = get(dagWithPreviews);
  return [...selected].flatMap(h => {
    const info = dag.get(h);
    return info === undefined ? [] : [info];
  });
});

export function useCommitSelection(hash: string): {
  isSelected: boolean;
  onClickToSelect: (
    _e: React.MouseEvent<HTMLDivElement> | React.KeyboardEvent<HTMLDivElement>,
  ) => unknown;
  overrideSelection: (newSelected: Array<Hash>) => void;
} {
  const isSelected = useAtomHas(selectedCommits, hash);
  const onClickToSelect = useCallback(
    (e: React.MouseEvent<HTMLDivElement> | React.KeyboardEvent<HTMLDivElement>) => {
      // previews won't change a commit from draft -> public, so we don't need
      // to use previews here
      const dag = readAtom(latestDag);
      if (dag.get(hash)?.phase === 'public' || hash === YOU_ARE_HERE_VIRTUAL_COMMIT.hash) {
        // don't bother selecting public commits / virtual commits
        return;
      }
      writeAtom(selectedCommits, last => {
        if (e.shiftKey) {
          const previouslySelected = readAtom(previouslySelectedCommit);
          if (previouslySelected != null) {
            let slice: Array<Hash> | null = null;
            const dag = readAtom(dagWithPreviews);
            // Prefer dag range for shift selection.
            const range = dag
              .range(hash, previouslySelected)
              .union(dag.range(previouslySelected, hash));
            if (range.size > 0) {
              slice = range.toArray();
            } else {
              // Fall back to displayed (flatten) range.
              const [sortIndex, sorted] = dag.defaultSortAscIndex();
              const prevIdx = sortIndex.get(previouslySelected);
              const nextIdx = sortIndex.get(hash);
              if (prevIdx != null && nextIdx != null) {
                const [fromIdx, toIdx] =
                  prevIdx > nextIdx ? [nextIdx, prevIdx] : [prevIdx, nextIdx];
                slice = sorted.slice(fromIdx, toIdx + 1);
              }
            }
            if (slice != null) {
              return new Set([...last, ...slice.filter(hash => dag.get(hash)?.phase !== 'public')]);
            }
          }
          // Holding shift, but we don't have a previous selected commit.
          // Fall through to treat it like a normal click.
        }

        const individualToggle = e[individualToggleKey];

        const selected = new Set(last);
        if (selected.has(hash)) {
          // multiple selected, then click an existing selected:
          //   if cmd, unselect just that one commit
          //   if not cmd, reset selection to just that one commit
          // only one selected, then click on it
          //   if cmd, unselect it
          //   it not cmd, unselect it
          if (!individualToggle && selected.size > 1) {
            // only select this commit
            selected.clear();
            selected.add(hash);
          } else {
            // unselect
            selected.delete(hash);
            writeAtom(previouslySelectedCommit, undefined);
          }
        } else {
          if (!individualToggle) {
            // clear if not holding cmd key
            selected.clear();
          }
          selected.add(hash);
        }
        return selected;
      });
      writeAtom(previouslySelectedCommit, hash);
    },
    [hash],
  );

  const overrideSelection = useCallback(
    (newSelected: Array<Hash>) => {
      // previews won't change a commit from draft -> public, so we don't need
      // to use previews here
      const dag = readAtom(latestDag);
      if (dag.get(hash)?.phase === 'public') {
        // don't bother selecting public commits
        return;
      }
      const nonPublicToSelect = newSelected.filter(hash => dag.get(hash)?.phase !== 'public');
      writeAtom(selectedCommits, new Set(nonPublicToSelect));
    },
    [hash],
  );

  return {isSelected, onClickToSelect, overrideSelection};
}

/** A richer version of `useCommitSelection`, provides extra handlers like `onDoubleClickToShowDrawer`. */
export function useCommitCallbacks(commit: CommitInfo): {
  isSelected: boolean;
  onClickToSelect: (
    _e: React.MouseEvent<HTMLDivElement> | React.KeyboardEvent<HTMLDivElement>,
  ) => unknown;
  onDoubleClickToShowDrawer: () => void;
} {
  const {isSelected, onClickToSelect, overrideSelection} = useCommitSelection(commit.hash);
  const onDoubleClickToShowDrawer = useCallback(() => {
    // Select the commit if it was deselected.
    if (!isSelected) {
      if (commit.hash === YOU_ARE_HERE_VIRTUAL_COMMIT.hash) {
        // don't select virutal commit, replace selection instead
        overrideSelection([]);
      } else {
        overrideSelection([commit.hash]);
      }
    }
    // Show the drawer.
    writeAtom(islDrawerState, state => ({
      ...state,
      right: {
        ...state.right,
        collapsed: false,
      },
    }));
    if (commit.isDot) {
      // if we happened to be in commit mode, swap to amend mode so you see the details instead
      writeAtom(commitMode, 'amend');
    }
  }, [overrideSelection, isSelected, commit.hash, commit.isDot]);
  return {isSelected, onClickToSelect, onDoubleClickToShowDrawer};
}

export function useArrowKeysToChangeSelection() {
  const cb = useCallback((which: ISLCommandName) => {
    if (which === 'OpenDetails') {
      writeAtom(islDrawerState, previous => ({
        ...previous,
        right: {
          ...previous.right,
          collapsed: false,
        },
      }));
    }

    const dag = readAtom(dagWithPreviews);
    const [sortIndex, sorted] = dag.defaultSortAscIndex();

    if (sorted.length === 0) {
      return;
    }

    const lastSelected = readAtom(previouslySelectedCommit);
    const lastIndex = lastSelected == null ? undefined : sortIndex.get(lastSelected);

    const nextSelectableHash = (step = 1 /* 1: up; -1: down */, start = lastIndex ?? 0) => {
      let index = start;
      while (index > 0) {
        index += step;
        const hash = sorted.at(index);
        if (hash == null) {
          return undefined;
        }
        // public commits are not selectable for now.
        if (dag.get(hash)?.phase !== 'public') {
          return hash;
        }
      }
    };

    const existingSelection = readAtom(selectedCommits);
    if (existingSelection.size === 0) {
      if (which === 'SelectDownwards' || which === 'ContinueSelectionDownwards') {
        const top = nextSelectableHash(-1, sorted.length);
        if (top != null) {
          writeAtom(selectedCommits, new Set([top]));
          writeAtom(previouslySelectedCommit, top);
        }
      }
      return;
    }

    if (lastSelected == null || lastIndex == null) {
      return;
    }

    let newSelected: Hash | undefined;
    let extendSelection = false;

    switch (which) {
      case 'SelectUpwards': {
        newSelected = nextSelectableHash(1);
        break;
      }
      case 'SelectDownwards': {
        newSelected = nextSelectableHash(-1);
        break;
      }
      case 'ContinueSelectionUpwards': {
        newSelected = nextSelectableHash(1);
        extendSelection = true;
        break;
      }
      case 'ContinueSelectionDownwards': {
        newSelected = nextSelectableHash(-1);
        extendSelection = true;
        break;
      }
    }

    if (newSelected != null) {
      const newHash = newSelected;
      writeAtom(selectedCommits, last =>
        extendSelection ? new Set([...last, newHash]) : new Set([newHash]),
      );
      writeAtom(previouslySelectedCommit, newHash);
    }
  }, []);

  useCommand('OpenDetails', () => cb('OpenDetails'));
  useCommand('SelectUpwards', () => cb('SelectUpwards'));
  useCommand('SelectDownwards', () => cb('SelectDownwards'));
  useCommand('ContinueSelectionUpwards', () => cb('ContinueSelectionUpwards'));
  useCommand('ContinueSelectionDownwards', () => cb('ContinueSelectionDownwards'));
  useSelectAllCommitsShortcut();
}

export function useBackspaceToHideSelected(): void {
  const cb = useCallback(() => {
    // Though you can select multiple commits, our preview system doens't handle that very well.
    // Just preview hiding the most recently selected commit.
    // Another sensible behavior would be to inspect the tree of commits selected
    // and find if there's a single common ancestor to hide. That won't work in all cases though.
    const mostRecent = readAtom(previouslySelectedCommit);
    let hashToHide = mostRecent;
    if (hashToHide == null) {
      const selection = readAtom(selectedCommits);
      if (selection != null) {
        hashToHide = firstOfIterable(selection.values());
      }
    }
    if (hashToHide == null) {
      return;
    }

    const commitToHide = readAtom(latestDag).get(hashToHide);
    if (commitToHide == null) {
      return;
    }

    writeAtom(
      operationBeingPreviewed,
      new HideOperation(latestSuccessorUnlessExplicitlyObsolete(commitToHide)),
    );
  }, []);

  useCommand('HideSelectedCommits', () => cb());
}
