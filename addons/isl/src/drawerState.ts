/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {AllDrawersState} from 'shared/Drawers';

import {persistAtomToLocalStorageEffect} from './persistAtomToConfigEffect';
import {getWindowWidthInPixels} from './utils';
import {atom} from 'recoil';

export const islDrawerState = atom<AllDrawersState>({
  key: 'islDrawerState',
  default: {
    right: {
      size: 500,
      // Collapse by default on small screens.
      collapsed: getWindowWidthInPixels() <= 500,
    },
    left: {size: 200, collapsed: true},
    top: {size: 200, collapsed: true},
    bottom: {size: 200, collapsed: true},
  },
  effects: [persistAtomToLocalStorageEffect('isl.drawer-state')],
});
