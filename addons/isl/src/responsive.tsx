/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {
  persistAtomToConfigEffect,
  persistAtomToLocalStorageEffect,
} from './persistAtomToConfigEffect';
import {useRef, useEffect} from 'react';
import {atom, selector, useSetRecoilState} from 'recoil';

export const mainContentWidthState = atom({
  key: 'mainContentWidthState',
  default: 500,
});

export const renderCompactAtom = atom<boolean>({
  key: 'renderCompactAtom',
  default: false,
  effects: [persistAtomToConfigEffect('isl.render-compact', false as boolean)],
});

const DEFAULT_FONT_SIZE = 16 as number;
export const fontSizeAtom = atom<number>({
  key: 'fontSizeAtom',
  default: DEFAULT_FONT_SIZE,
  effects: [
    persistAtomToLocalStorageEffect('isl.font-size'),
    ({onSet, getLoadable}) => {
      const set = (value: number) => document.body.style.setProperty('--font-size-raw', `${value}`);

      // Set font size immediately
      const val = getLoadable(fontSizeAtom).valueMaybe();
      if (val != null) {
        set(val);
      }

      onSet((newValue, _oldValue) => {
        set(newValue);
      });
    },
  ],
});

export function useMainContentWidth() {
  const setMainContentWidth = useSetRecoilState(mainContentWidthState);

  const mainContentRef = useRef<null | HTMLDivElement>(null);
  useEffect(() => {
    const element = mainContentRef.current;
    if (element == null) {
      return;
    }

    const obs = new ResizeObserver(entries => {
      const [entry] = entries;
      setMainContentWidth(entry.contentRect.width);
    });
    obs.observe(element);
    return () => obs.unobserve(element);
  }, [mainContentRef, setMainContentWidth]);

  return mainContentRef;
}

export const NARROW_COMMIT_TREE_WIDTH = 800;
export const NARROW_COMMIT_TREE_WIDTH_WHEN_COMPACT = 300;

export const isNarrowCommitTree = selector({
  key: 'isNarrowCommitTree',
  get: ({get}) =>
    get(mainContentWidthState) <
    (get(renderCompactAtom) ? NARROW_COMMIT_TREE_WIDTH_WHEN_COMPACT : NARROW_COMMIT_TREE_WIDTH),
});
