/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {MutAtom} from './jotaiUtils';
import type {AtomEffect, MutableSnapshot, RecoilState} from 'recoil';

import {globalRecoil} from './AccessGlobalRecoil';
import serverAPI from './ClientToServerAPI';
import {atomWithOnChange, writeAtom} from './jotaiUtils';
import {atom} from 'jotai';
import {atom as RecoilAtom} from 'recoil';

/**
 * Atom effect that clears the atom's value when the current working directory / repository changes.
 */
export function clearOnCwdChange<T>(): AtomEffect<T> {
  return ({resetSelf}) => serverAPI.onCwdChanged(resetSelf);
}

const entangledAtomsInitializedState: Map<RecoilState<unknown>, unknown> = new Map();

export function getEntangledAtomsInitializedState(snapshot: MutableSnapshot): void {
  for (const [atom, value] of entangledAtomsInitializedState.entries()) {
    snapshot.set(atom, value);
  }
  // Note: we intentionally don't clear the map, since RecoilRoot is sometimes re-called during hot reloads.
}

/**
 * Creates a pair of Jotai and Recoil atoms that is "entangled".
 * Changing one atom automatically updates the other.
 */
export function entangledAtoms<T>(props: {
  default: T;
  key: string;
  effects?: AtomEffect<T>[];
}): [MutAtom<T>, RecoilState<T>] {
  const initialValue = props.default;
  const {key} = props;

  // This is a private atom so this function is the only place to update it.
  // Updating from elsewhere won't trigger the Jotai->Recoil sync.
  const originalAtom = atom<T>(initialValue);

  let recoilValue = initialValue;
  let jotaiValue = initialValue;

  const effects = props.effects ?? [];
  effects.push(({onSet}) => {
    onSet(newValue => {
      if (jotaiValue !== newValue) {
        // Jotai value is outdated.
        jotaiValue = newValue;
        writeAtom(originalAtom, newValue);
      }
    });
  });
  const recoilAtom = RecoilAtom<T>({
    key,
    default: initialValue,
    effects,
  });

  const jotaiAtom = atomWithOnChange(originalAtom, value => {
    if (recoilValue !== value) {
      // Recoil value is outdated.
      recoilValue = value;
      const recoil = globalRecoil();
      if (recoil == null) {
        // Sometimes, an atom is written to before RecoilRoot is initialized.
        // Save such values so we can pass them to RecoilRoot's initializeState..
        entangledAtomsInitializedState.set(recoilAtom as RecoilState<unknown>, value);
      } else {
        recoil.set(recoilAtom, value);
      }
    }
  });
  jotaiAtom.debugLabel = key;

  return [jotaiAtom, recoilAtom];
}
