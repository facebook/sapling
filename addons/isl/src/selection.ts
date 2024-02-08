/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ISLCommandName} from './ISLShortcuts';
import type {CommitInfo, Hash} from './types';
import type React from 'react';

import {useCommand} from './ISLShortcuts';
import {useSelectAllCommitsShortcut} from './SelectAllCommits';
import {latestSuccessorUnlessExplicitlyObsolete, successionTracker} from './SuccessionTracker';
import {islDrawerState} from './drawerState';
import {readAtom, writeAtom} from './jotaiUtils';
import {HideOperation} from './operations/HideOperation';
import {dagWithPreviews} from './previews';
import {entangledAtoms} from './recoilUtils';
import {latestDag, operationBeingPreviewed} from './serverAPIState';
import {firstOfIterable, registerCleanup} from './utils';
import {atom} from 'jotai';
import {selector, selectorFamily, useRecoilCallback, useRecoilValue} from 'recoil';

/**
 * See {@link selectedCommitInfos}
 * Note: it is possible to be selecting a commit that stops being rendered, and thus has no associated commit info.
 * Prefer to use `selectedCommitInfos` to get the subset of the selection that is visible.
 */
export const [selectedCommits, selectedCommitsRecoil] = entangledAtoms<Set<Hash>>({
  key: 'selectedCommits',
  default: new Set(),
});
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

export const isCommitSelected = selectorFamily({
  key: 'isCommitSelected',
  get:
    (hash: Hash) =>
    ({get}) =>
      get(selectedCommitsRecoil).has(hash),
});

const previouslySelectedCommit = atom<undefined | string>(undefined);

/**
 * Clicking on commits will select them in the UI.
 * Selected commits can be acted on in bulk, and appear in the commit info sidebar for editing / details.
 * Invariant: Selected commits are non-public.
 *
 * See {@link selectedCommits} for setting underlying storage
 */
export const selectedCommitInfos = selector<Array<CommitInfo>>({
  key: 'selectedCommitInfos',
  get: ({get}) => {
    const selected = get(selectedCommitsRecoil);
    const dag = get(dagWithPreviews);
    return [...selected].flatMap(h => {
      const info = dag.get(h);
      return info === undefined ? [] : [info];
    });
  },
});

export function useCommitSelection(hash: string): {
  isSelected: boolean;
  onClickToSelect: (
    _e: React.MouseEvent<HTMLDivElement> | React.KeyboardEvent<HTMLDivElement>,
  ) => unknown;
  overrideSelection: (newSelected: Array<Hash>) => void;
} {
  const isSelected = useRecoilValue(isCommitSelected(hash));
  const onClickToSelect = useRecoilCallback(
    ({snapshot}) =>
      (e: React.MouseEvent<HTMLDivElement> | React.KeyboardEvent<HTMLDivElement>) => {
        // previews won't change a commit from draft -> public, so we don't need
        // to use previews here
        const loadable = snapshot.getLoadable(latestDag);
        if (loadable.getValue().get(hash)?.phase === 'public') {
          // don't bother selecting public commits
          return;
        }
        writeAtom(selectedCommits, last => {
          if (e.shiftKey) {
            const previouslySelected = readAtom(previouslySelectedCommit);
            const linearHistory = snapshot.getLoadable(linearizedCommitHistory).valueMaybe();
            if (linearHistory != null && previouslySelected != null) {
              const prevIdx = linearHistory.findIndex(val => val.hash === previouslySelected);
              const nextIdx = linearHistory.findIndex(val => val.hash === hash);

              const [fromIdx, toIdx] = prevIdx > nextIdx ? [nextIdx, prevIdx] : [prevIdx, nextIdx];
              const slice = linearHistory.slice(fromIdx, toIdx + 1);

              return new Set([
                ...last,
                ...slice.filter(commit => commit.phase !== 'public').map(commit => commit.hash),
              ]);
            } else {
              // Holding shift, but we don't have a previous selected commit.
              // Fall through to treat it like a normal click.
            }
          }

          const selected = new Set(last);
          if (selected.has(hash)) {
            // multiple selected, then click an existing selected:
            //   if cmd, unselect just that one commit
            //   if not cmd, reset selection to just that one commit
            // only one selected, then click on it
            //   if cmd, unselect it
            //   it not cmd, unselect it
            if (!e.metaKey && selected.size > 1) {
              // only select this commit
              selected.clear();
              selected.add(hash);
            } else {
              // unselect
              selected.delete(hash);
              writeAtom(previouslySelectedCommit, undefined);
            }
          } else {
            if (!e.metaKey) {
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

  const overrideSelection = useRecoilCallback(
    ({snapshot}) =>
      (newSelected: Array<Hash>) => {
        // previews won't change a commit from draft -> public, so we don't need
        // to use previews here
        const loadable = snapshot.getLoadable(latestDag);
        if (loadable.getValue().get(hash)?.phase === 'public') {
          // don't bother selecting public commits
          return;
        }
        const nonPublicToSelect = newSelected.filter(
          hash => loadable.getValue().get(hash)?.phase !== 'public',
        );
        writeAtom(selectedCommits, new Set(nonPublicToSelect));
      },
    [hash],
  );

  return {isSelected, onClickToSelect, overrideSelection};
}

/**
 * Convert commit tree to linear history, where commits are neighbors in the array
 * if they are visually next to each other when rendered as a tree
 * c            c
 * b            b
 * | e    ->    e
 * | d          d
 * |/           a
 * a
 * in bottom to top order: [a,d,e,b,c]
 */
export const linearizedCommitHistory = selector({
  key: 'linearizedCommitHistory',
  get: ({get}) => {
    const dag = get(dagWithPreviews);
    const sorted: Hash[] = dag.sortAsc(dag, {gap: false});
    return dag.getBatch(sorted);
  },
});

export function useArrowKeysToChangeSelection() {
  const cb = useRecoilCallback(({snapshot}) => (which: ISLCommandName) => {
    if (which === 'OpenDetails') {
      writeAtom(islDrawerState, previous => ({
        ...previous,
        right: {
          ...previous.right,
          collapsed: false,
        },
      }));
    }

    const linearHistory = snapshot.getLoadable(linearizedCommitHistory).valueMaybe();
    if (linearHistory == null || linearHistory.length === 0) {
      return;
    }

    const linearNonPublicHistory = linearHistory.filter(commit => commit.phase !== 'public');

    const existingSelection = readAtom(selectedCommits);
    if (existingSelection.size === 0) {
      if (which === 'SelectDownwards' || which === 'ContinueSelectionDownwards') {
        const top = linearNonPublicHistory.at(-1)?.hash;
        if (top != null) {
          writeAtom(selectedCommits, new Set([top]));
          writeAtom(previouslySelectedCommit, top);
        }
      }
      return;
    }

    const lastSelected = readAtom(previouslySelectedCommit);
    if (lastSelected == null) {
      return;
    }

    let currentIndex = linearNonPublicHistory.findIndex(commit => commit.hash === lastSelected);
    if (currentIndex === -1) {
      return;
    }

    let extendSelection = false;

    switch (which) {
      case 'SelectUpwards': {
        if (currentIndex < linearNonPublicHistory.length - 1) {
          currentIndex++;
        }
        break;
      }
      case 'SelectDownwards': {
        if (currentIndex > 0) {
          currentIndex--;
        }
        break;
      }
      case 'ContinueSelectionUpwards': {
        if (currentIndex < linearNonPublicHistory.length - 1) {
          currentIndex++;
        }
        extendSelection = true;
        break;
      }
      case 'ContinueSelectionDownwards': {
        if (currentIndex > 0) {
          currentIndex--;
        }
        extendSelection = true;
        break;
      }
    }

    const newSelected = linearNonPublicHistory[currentIndex];
    writeAtom(selectedCommits, last =>
      extendSelection ? new Set([...last, newSelected.hash]) : new Set([newSelected.hash]),
    );
    writeAtom(previouslySelectedCommit, newSelected.hash);
  });

  useCommand('OpenDetails', () => cb('OpenDetails'));
  useCommand('SelectUpwards', () => cb('SelectUpwards'));
  useCommand('SelectDownwards', () => cb('SelectDownwards'));
  useCommand('ContinueSelectionUpwards', () => cb('ContinueSelectionUpwards'));
  useCommand('ContinueSelectionDownwards', () => cb('ContinueSelectionDownwards'));
  useSelectAllCommitsShortcut();
}

export function useBackspaceToHideSelected(): void {
  const cb = useRecoilCallback(({snapshot, set}) => () => {
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

    const loadable = snapshot.getLoadable(latestDag);
    const commitToHide = loadable.getValue().get(hashToHide);
    if (commitToHide == null) {
      return;
    }

    set(
      operationBeingPreviewed,
      new HideOperation(latestSuccessorUnlessExplicitlyObsolete(commitToHide)),
    );
  });

  useCommand('HideSelectedCommits', () => cb());
}
