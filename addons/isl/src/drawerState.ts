/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {AllDrawersState, DrawerState} from 'shared/Drawers';

import {persistAtomToLocalStorageEffect} from './persistAtomToConfigEffect';
import {getWindowWidthInPixels} from './utils';
import {DefaultValue, atom} from 'recoil';

const AUTO_CLOSE_MAX_SIZE = 700;
const DEFAULT_RIGHT_DRAWER_WIDTH = 500;

export const islDrawerState = atom<AllDrawersState>({
  key: 'islDrawerState',
  default: {
    right: {
      size: DEFAULT_RIGHT_DRAWER_WIDTH,
      collapsed: false,
    },
    left: {size: 200, collapsed: true},
    top: {size: 200, collapsed: true},
    bottom: {size: 200, collapsed: true},
  },
  effects: [
    persistAtomToLocalStorageEffect('isl.drawer-state'),
    ({setSelf, getLoadable}) => {
      // On startup, override existing state to collapse the right sidebar if the screen is too small.
      // This allows collapsing even if the size has been previous persisted.
      function autoCloseBasedOnWindowWidth() {
        const windowWidth = getWindowWidthInPixels();

        const current = getLoadable(islDrawerState).valueMaybe()?.right.size;
        const setDrawer = (state: DrawerState) => {
          setSelf(oldValue => {
            if (oldValue instanceof DefaultValue) {
              return oldValue;
            }
            return {
              ...oldValue,
              right: state,
            };
          });
        };
        if (windowWidth < AUTO_CLOSE_MAX_SIZE) {
          setDrawer({
            collapsed: true,
            size: Math.min(windowWidth, current ?? DEFAULT_RIGHT_DRAWER_WIDTH),
          });
        }
      }
      // check startup window size
      autoCloseBasedOnWindowWidth();

      const resizeFn = () => {
        autoCloseBasedOnWindowWidth();
      };
      window.addEventListener('resize', resizeFn);
      return () => window.removeEventListener('resize', resizeFn);
    },
  ],
});
