/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {createStore} from 'jotai';
import {__TEST__, islDrawerState} from '../drawerState';
import {readAtom, setJotaiStore, writeAtom} from '../jotaiUtils';

const openDrawerState = {
  right: {size: 500, collapsed: false},
  left: {size: 200, collapsed: true},
  top: {size: 200, collapsed: true},
  bottom: {size: 200, collapsed: true},
};

describe('drawer state', () => {
  beforeEach(() => {
    setJotaiStore(createStore());
    writeAtom(islDrawerState, openDrawerState);
  });

  it('preserves an open drawer when the page loads hidden', () => {
    __TEST__.autoCloseBasedOnWindowWidth(500, 'hidden');

    expect(readAtom(islDrawerState).right.collapsed).toBe(false);
  });

  it('auto-closes an open horizontal drawer when the visible page is narrow', () => {
    __TEST__.autoCloseBasedOnWindowWidth(500, 'visible');

    expect(readAtom(islDrawerState).right).toEqual({size: 500, collapsed: true});
  });
});
