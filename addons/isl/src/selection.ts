/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type React from 'react';

import {latestCommitTreeMap} from './serverAPIState';
import {atom, useRecoilCallback, useRecoilValue} from 'recoil';

/**
 * Clicking on commits will select them in the UI.
 * Selected commits can be acted on in bulk, and appear in the commit info sidebar for editing / details.
 * Invariant: Selected commits are non-public.
 */
export const selectedCommits = atom<Set<string>>({
  key: 'selectedCommits',
  default: new Set(),
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
      (_e: React.MouseEvent<HTMLDivElement> | React.KeyboardEvent<HTMLDivElement>) => {
        // TODO: cmd-click, shift-click to select multiple.
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
            selected.delete(hash);
          } else {
            selected.clear();
            selected.add(hash);
          }
          return selected;
        });
      },
    [hash],
  );
  return {isSelected: selected.has(hash), onClickToSelect};
}
