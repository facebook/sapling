/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {AllDrawersState, DrawerState} from './Drawers';

import {localStorageBackedAtom, readAtom, writeAtom} from './jotaiUtils';
import {getWindowWidthInPixels, registerCleanup} from './utils';

const AUTO_CLOSE_MAX_SIZE = 700;
const DEFAULT_RIGHT_DRAWER_WIDTH = 500;

export const islDrawerState = localStorageBackedAtom<AllDrawersState>('isl.drawer-state', {
  right: {
    size: DEFAULT_RIGHT_DRAWER_WIDTH,
    collapsed: false,
  },
  left: {size: 200, collapsed: true},
  top: {size: 200, collapsed: true},
  bottom: {size: 200, collapsed: true},
});

// On startup, override existing state to collapse the right sidebar if the screen is too small.
// This allows collapsing even if the size has been previous persisted.
function autoCloseBasedOnWindowWidth() {
  const windowWidth = getWindowWidthInPixels();
  if (windowWidth === 0) {
    // window not loaded yet
    return;
  }

  const current = readAtom(islDrawerState).right.size;
  const setDrawer = (state: DrawerState) => {
    const oldValue = readAtom(islDrawerState);
    writeAtom(islDrawerState, {
      ...oldValue,
      right: state,
    });
  };
  if (windowWidth < AUTO_CLOSE_MAX_SIZE) {
    setDrawer({
      collapsed: true,
      size: Math.min(windowWidth, current ?? DEFAULT_RIGHT_DRAWER_WIDTH),
    });
  }
}

const resizeFn = () => {
  autoCloseBasedOnWindowWidth();
};
window.addEventListener('resize', resizeFn);

// check startup window size
window.addEventListener('load', resizeFn);

registerCleanup(
  islDrawerState,
  () => {
    window.removeEventListener('resize', resizeFn);
    window.removeEventListener('load', resizeFn);
  },
  import.meta.hot,
);
