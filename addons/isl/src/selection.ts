/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from './types';
import type React from 'react';

import {treeWithPreviews} from './previews';
import {latestCommitTreeMap} from './serverAPIState';
import {atom, selector, useRecoilCallback, useRecoilValue} from 'recoil';
import {notEmpty} from 'shared/utils';

/**
 * See {@link selectedCommitInfos}
 * Note: it is possible to be selecting a commit that stops being rendered, and thus has no associated commit info.
 * Prefer to use `selectedCommitInfos` to get the subset of the selection that is visible.
 */
export const selectedCommits = atom<Set<string>>({
  key: 'selectedCommits',
  default: new Set(),
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
    const {treeMap} = get(treeWithPreviews);
    const commits = [...selected]
      .map(hash => {
        const tree = treeMap.get(hash);
        if (tree == null) {
          return null;
        }
        return tree.info;
      })
      .filter(notEmpty);
    return commits;
  },
});

export function useCommitSelection(hash: string): {
  isSelected: boolean;
  onClickToSelect: (
    _e: React.MouseEvent<HTMLDivElement> | React.KeyboardEvent<HTMLDivElement>,
  ) => unknown;
} {
  const selected = useRecoilValue(selectedCommits);
  const onClickToSelect = useRecoilCallback(
    ({set, snapshot}) =>
      (e: React.MouseEvent<HTMLDivElement> | React.KeyboardEvent<HTMLDivElement>) => {
        // previews won't change a commit from draft -> public, so we don't need
        // to use previews here
        const loadable = snapshot.getLoadable(latestCommitTreeMap);
        if (loadable.getValue().get(hash)?.info.phase === 'public') {
          // don't bother selecting public commits
          return;
        }
        set(selectedCommits, last => {
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
      },
    [hash],
  );
  return {isSelected: selected.has(hash), onClickToSelect};
}
