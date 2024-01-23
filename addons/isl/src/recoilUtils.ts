/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {MutAtom} from './jotaiUtils';
import type {PrimitiveAtom} from 'jotai';
import type {AtomEffect, RecoilState} from 'recoil';

import {globalRecoil} from './AccessGlobalRecoil';
import serverAPI from './ClientToServerAPI';
import {atomWithOnChange, readAtom, writeAtom} from './jotaiUtils';
import {atom as RecoilAtom} from 'recoil';

/**
 * Atom effect that clears the atom's value when the current working directory / repository changes.
 */
export function clearOnCwdChange<T>(): AtomEffect<T> {
  return ({resetSelf}) => serverAPI.onCwdChanged(resetSelf);
}

/**
 * Creates a pair of Jotai and Recoil atoms that is "entangled".
 * Changing one atom automatically updates the other.
 *
 * The "source of truth" is the "originalAtom". Note the "originalAtom"
 * should not be updated without going through the returned Jotai atom.
 */
export function entangledAtoms<T>(
  originalAtom: PrimitiveAtom<T>,
  key: string,
  recoilEffects?: AtomEffect<T>[],
): [MutAtom<T>, RecoilState<T>] {
  const initialValue = readAtom(originalAtom);

  let recoilValue = initialValue;
  let jotaiValue = initialValue;

  const jotaiAtom = atomWithOnChange(originalAtom, value => {
    if (recoilValue !== value) {
      // Recoil value is outdated.
      recoilValue = value;
      globalRecoil().set(recoilAtom, value);
    }
  });
  jotaiAtom.debugLabel = key;

  const effects = recoilEffects ?? [];
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

  return [jotaiAtom, recoilAtom];
}
