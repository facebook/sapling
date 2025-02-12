/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {atom, useSetAtom} from 'jotai';
import {useEffect, useRef} from 'react';
import {useCommand} from './ISLShortcuts';
import {
  atomWithOnChange,
  configBackedAtom,
  localStorageBackedAtom,
  readAtom,
  writeAtom,
} from './jotaiUtils';

export const mainContentWidthState = atom(500);

export const renderCompactAtom = configBackedAtom<boolean>('isl.render-compact', false);

export const zoomUISettingAtom = atomWithOnChange(
  localStorageBackedAtom<number>('isl.ui-zoom', 1),
  newValue => {
    document.body?.style.setProperty('--zoom', `${newValue}`);
  },
);

export function useZoomShortcut() {
  useCommand('ZoomIn', () => {
    const old = readAtom(zoomUISettingAtom);
    writeAtom(zoomUISettingAtom, Math.round((old + 0.1) * 100) / 100);
  });
  useCommand('ZoomOut', () => {
    const old = readAtom(zoomUISettingAtom);
    writeAtom(zoomUISettingAtom, Math.round((old - 0.1) * 100) / 100);
  });
}

export function useMainContentWidth() {
  const setMainContentWidth = useSetAtom(mainContentWidthState);

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

export const isNarrowCommitTree = atom(
  get =>
    get(mainContentWidthState) <
    (get(renderCompactAtom) ? NARROW_COMMIT_TREE_WIDTH_WHEN_COMPACT : NARROW_COMMIT_TREE_WIDTH),
);
