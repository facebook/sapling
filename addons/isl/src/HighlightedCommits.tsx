/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo, Hash} from './types';

import {atom, useSetAtom} from 'jotai';
import {useEffect, useState} from 'react';
import {atomFamilyWeak} from './jotaiUtils';

export const highlightedCommits = atom<Set<Hash>>(new Set<Hash>());

export const isHighlightedCommit = atomFamilyWeak((hash: Hash) =>
  atom(get => get(highlightedCommits).has(hash)),
);

export function HighlightCommitsWhileHovering({
  toHighlight,
  children,
  ...rest
}: {
  toHighlight: Array<CommitInfo | Hash>;
  children: React.ReactNode;
} & React.DetailedHTMLProps<React.HTMLAttributes<HTMLDivElement>, HTMLDivElement>) {
  const setHighlighted = useSetAtom(highlightedCommits);
  const [isSourceOfHighlight, setIsSourceOfHighlight] = useState(false);

  useEffect(() => {
    return () => {
      if (isSourceOfHighlight) {
        // if we started the highlight, make sure to unhighlight when unmounting
        setHighlighted(new Set());
      }
    };
  }, [isSourceOfHighlight, setHighlighted]);

  return (
    <div
      {...rest}
      onMouseOver={() => {
        setHighlighted(
          new Set(
            toHighlight.map(commitOrHash =>
              typeof commitOrHash === 'string' ? commitOrHash : commitOrHash.hash,
            ),
          ),
        );
        setIsSourceOfHighlight(true);
      }}
      onMouseOut={() => {
        setHighlighted(new Set());
        setIsSourceOfHighlight(false);
      }}>
      {children}
    </div>
  );
}
