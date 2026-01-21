/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {AllDrawersState} from './Drawers';

import {atom} from 'jotai';
import {localStorageBackedAtom} from './jotaiUtils';

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

/**
 * Tracks whether each drawer was auto-collapsed by the responsive system.
 * Used to distinguish between manual collapse (user clicked) and auto-collapse (window too narrow).
 * - When auto-collapsed: drawer will auto-expand when window widens
 * - When manually collapsed: drawer stays collapsed regardless of window size
 */
export const autoCollapsedState = atom<{right: boolean; left: boolean}>({
  right: false,
  left: false,
});
