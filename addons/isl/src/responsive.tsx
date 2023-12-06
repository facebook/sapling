/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {globalRecoil} from './AccessGlobalRecoil';
import {useCommand} from './ISLShortcuts';
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

export const zoomUISettingAtom = atom<number>({
  key: 'zoomUISettingAtom',
  default: 1.0,
  effects: [
    persistAtomToLocalStorageEffect('isl.ui-zoom'),
    ({onSet, getLoadable}) => {
      const initial = getLoadable(zoomUISettingAtom).valueMaybe();
      if (initial != null) {
        document.body?.style.setProperty('--zoom', `${initial}`);
      }
      onSet(newValue => {
        document.body?.style.setProperty('--zoom', `${newValue}`);
      });
    },
  ],
});

export function useZoomShortcut() {
  useCommand('ZoomIn', () => {
    globalRecoil().set(zoomUISettingAtom, old => Math.round((old + 0.1) * 100) / 100);
  });
  useCommand('ZoomOut', () => {
    globalRecoil().set(zoomUISettingAtom, old => Math.round((old - 0.1) * 100) / 100);
  });
}

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
