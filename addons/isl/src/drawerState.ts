/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {AllDrawersState} from './Drawers';

import {localStorageBackedAtom} from './jotaiUtils';

const DEFAULT_LEFT_DRAWER_WIDTH = 376;
const DEFAULT_RIGHT_DRAWER_WIDTH = 380;

// v8 key to reset cached drawer sizes with new defaults (left 376px expanded, right collapsed)
export const islDrawerState = localStorageBackedAtom<AllDrawersState>('isl.drawer-state-v8', {
  right: {
    size: DEFAULT_RIGHT_DRAWER_WIDTH,
    collapsed: true,
  },
  left: {size: DEFAULT_LEFT_DRAWER_WIDTH, collapsed: false},
  top: {size: 200, collapsed: true},
  bottom: {size: 200, collapsed: true},
});
