/**
 * Portions Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/*
This is inspired by the recoil-nexus project: https://github.com/luisanton-io/recoil-nexus
MIT License
Copyright (c) 2021 Luis Antonio Canettoli Ordoñez

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

import type {Loadable, RecoilState, RecoilValue, Snapshot} from 'recoil';

import {
  useGetRecoilValueInfo_UNSTABLE,
  useRecoilTransaction_UNSTABLE,
  useRecoilCallback,
} from 'recoil';

export type GlobalRecoilAccess = {
  getLoadable: <T>(atom: RecoilValue<T>) => Loadable<T>;
  set: <T>(atom: RecoilState<T>, valOrUpdater: T | ((currVal: T) => T)) => void;
  reset: <T>(atom: RecoilState<T>) => void;
  getSnapshot: () => Snapshot;
};

/**
 * You can technically have multiple recoil roots, index them by a key.
 * The main root is automatically saved under "default".
 */
const roots: Record<string, GlobalRecoilAccess> = {};

/**
 * Expose accessors to get/set recoil state, even outside of the recoil context.
 * This is useful to allow atoms to get/set each other, or to manipulate recoil state
 * from some subscription outside of recoil.
 */
export function globalRecoil(name?: string): GlobalRecoilAccess {
  return roots[name ?? 'default'];
}

/**
 * Expose accessors to get/set recoil state, even outside of the recoil context.
 * This is useful to allow atoms to get/set each other, or to manipulate recoil state
 * from some subscription outside of recoil.
 */
export function AccessGlobalRecoil({name}: {name?: string}) {
  const recoil: Partial<GlobalRecoilAccess> = {};

  const getInfo = useGetRecoilValueInfo_UNSTABLE();
  const transact = useRecoilTransaction_UNSTABLE(({set}) => set);

  recoil.getLoadable = useRecoilCallback(
    ({snapshot}) =>
      <T,>(atom: RecoilValue<T>) =>
        snapshot.getLoadable(atom),
  ) as <T>(atom: RecoilValue<T>) => Loadable<T>;

  recoil.set = useRecoilCallback(
    ({set}) =>
      <T,>(atom: RecoilState<T>, valOrUpdate: T | ((currVal: T) => T)) => {
        switch (getInfo(atom).type) {
          case 'atom':
            return transact(atom, valOrUpdate);
          case 'selector':
            return set(atom, valOrUpdate);
        }
      },
  ) as <T>(atom: RecoilState<T>, valOrUpdater: T | ((currVal: T) => T)) => void;

  recoil.reset = useRecoilCallback(({reset}) => reset, []);

  recoil.getSnapshot = useRecoilCallback(
    ({snapshot}) =>
      () =>
        snapshot,
  );

  roots[name ?? 'default'] = recoil as GlobalRecoilAccess;

  return null;
}
