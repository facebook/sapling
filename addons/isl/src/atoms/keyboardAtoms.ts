/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {writeAtom} from '../jotaiUtils';
import {registerCleanup} from '../utils';
import {atom} from 'jotai';

/** Subset of KeyboardEvent. */
export type KeyPress = {
  altKey?: boolean;
  ctrlKey?: boolean;
  shiftKey?: boolean;
  metaKey?: boolean;
  isComposing?: boolean;
};

/** State of if modified keys (alt, ctrl, etc) are currently pressed. */
export const keyPressAtom = atom<KeyPress>({});

const keyChange = (e: KeyboardEvent) => {
  const {altKey, ctrlKey, shiftKey, metaKey, isComposing} = e;
  writeAtom(keyPressAtom, {altKey, ctrlKey, shiftKey, metaKey, isComposing});
};
document.addEventListener('keydown', keyChange);
document.addEventListener('keyup', keyChange);

registerCleanup(
  keyPressAtom,
  () => {
    document.removeEventListener('keydown', keyChange);
    document.removeEventListener('keyup', keyChange);
  },
  import.meta.hot,
);

/** Is the alt key currently held down. */
export const holdingAltAtom = atom<boolean>(get => get(keyPressAtom).altKey ?? false);

/** Is the ctrl key currently held down. */
export const holdingCtrlAtom = atom<boolean>(get => get(keyPressAtom).ctrlKey ?? false);

/** Is the meta ("Command" on macOS, or "Windows" on Windows) key currently held down. */
export const holdingMetaKey = atom<boolean>(get => get(keyPressAtom).metaKey ?? false);

/** Is the shift key currently held down. */
export const holdingShiftAtom = atom<boolean>(get => get(keyPressAtom).shiftKey ?? false);
