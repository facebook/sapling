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
import {HideOperation} from './operations/HideOperation';
import {dagWithPreviews, treeWithPreviews} from './previews';
import {latestDag, operationBeingPreviewed} from './serverAPIState';
import {firstOfIterable} from './utils';
import {atom, selector, useRecoilCallback, useRecoilValue} from 'recoil';

/**
 * See {@link selectedCommitInfos}
 * Note: it is possible to be selecting a commit that stops being rendered, and thus has no associated commit info.
 * Prefer to use `selectedCommitInfos` to get the subset of the selection that is visible.
 */
export const selectedCommits = atom<Set<Hash>>({
  key: 'selectedCommits',
  default: new Set(),
  effects: [
    ({setSelf, getLoadable}) => {
      return successionTracker.onSuccessions(successions => {
        const value = new Set(getLoadable(selectedCommits).valueMaybe());
        for (const [oldHash, newHash] of successions) {
          if (value?.has(oldHash)) {
            value.delete(oldHash);
            value.add(newHash);
          }
        }
        setSelf(value);
      });
    },
  ],
});

const previouslySelectedCommit = atom<undefined | string>({
  key: 'previouslySelectedCommit',
  default: undefined,
});

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
    const selected = get(selectedCommits);
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
  const selected = useRecoilValue(selectedCommits);
  const onClickToSelect = useRecoilCallback(
    ({set, snapshot}) =>
      (e: React.MouseEvent<HTMLDivElement> | React.KeyboardEvent<HTMLDivElement>) => {
        // previews won't change a commit from draft -> public, so we don't need
        // to use previews here
        const loadable = snapshot.getLoadable(latestDag);
        if (loadable.getValue().get(hash)?.phase === 'public') {
          // don't bother selecting public commits
          return;
        }
        set(selectedCommits, last => {
          if (e.shiftKey) {
            const previouslySelected = snapshot.getLoadable(previouslySelectedCommit).valueMaybe();
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
              set(previouslySelectedCommit, undefined);
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
        set(previouslySelectedCommit, hash);
      },
    [hash],
  );

  const overrideSelection = useRecoilCallback(
    ({set, snapshot}) =>
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
        set(selectedCommits, new Set(nonPublicToSelect));
      },
    [hash],
  );

  return {isSelected: selected.has(hash), onClickToSelect, overrideSelection};
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
    const {trees} = get(treeWithPreviews);

    const toProcess = [...trees];
    const accum = [];

    while (toProcess.length > 0) {
      const next = toProcess.pop();
      if (!next) {
        break;
      }

      accum.push(next.info);
      toProcess.push(...next.children);
    }

    return accum;
  },
});

export function useArrowKeysToChangeSelection() {
  const cb = useRecoilCallback(({snapshot, set}) => (which: ISLCommandName) => {
    if (which === 'OpenDetails') {
      set(islDrawerState, previous => ({
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

    const existingSelection = snapshot.getLoadable(selectedCommits).valueMaybe();
    if (existingSelection == null || existingSelection.size === 0) {
      if (which === 'SelectDownwards' || which === 'ContinueSelectionDownwards') {
        const top = linearNonPublicHistory.at(-1)?.hash;
        if (top != null) {
          set(selectedCommits, new Set([top]));
          set(previouslySelectedCommit, top);
        }
      }
      return;
    }

    const lastSelected = snapshot.getLoadable(previouslySelectedCommit).valueMaybe();
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
    set(selectedCommits, last =>
      extendSelection ? new Set([...last, newSelected.hash]) : new Set([newSelected.hash]),
    );
    set(previouslySelectedCommit, newSelected.hash);
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
    const mostRecent = snapshot.getLoadable(previouslySelectedCommit).valueMaybe();
    let hashToHide = mostRecent;
    if (hashToHide == null) {
      const selection = snapshot.getLoadable(selectedCommits).valueMaybe();
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
