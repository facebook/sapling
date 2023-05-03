/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from './types';

import clientToServerAPI from './ClientToServerAPI';
import {CommitStackState} from './stackEdit/commitStackState';
import {atom, DefaultValue, selector} from 'recoil';

/** State related to stack editing UI. */
type StackEditState = {
  /**
   * Commit hashes being edited.
   * Empty means no editing is requested.
   *
   * Changing this to a non-empty value triggers `exportStack`
   * message to the server.
   */
  hashes: Set<Hash>;

  /**
   * The (mutable) main stack state.
   */
  stack: Loading<CommitStackState>;
};

/** Lightweight recoil Loadable alternative that is not coupled with Promise. */
export type Loading<T> =
  | {state: 'loading'}
  | {state: 'hasValue'; value: T}
  | {state: 'hasError'; error: string};

// This is private so we can maintain state consistency
// (ex. stack and requested hashes cannot be out of sync)
// via selectors.
const stackEditState = atom<StackEditState>({
  key: 'stackEditState',
  default: {
    hashes: new Set<Hash>(),
    stack: {state: 'loading'},
  },
  effects: [
    ({setSelf}) => {
      // Listen to the exportedStack event.
      const disposable = clientToServerAPI.onMessageOfType('exportedStack', event => {
        setSelf(prev => {
          const hashes = prev instanceof DefaultValue ? new Set<Hash>() : prev.hashes;
          const revs = getRevs(hashes);
          if (revs !== event.revs) {
            // Wrong stack. Ignore it.
            return prev;
          }
          if (event.error != null) {
            return {hashes, stack: {state: 'hasError', error: event.error}};
          } else {
            try {
              const stack = new CommitStackState(event.stack);
              return {hashes, stack: {state: 'hasValue', value: stack}};
            } catch (err) {
              const msg = `Cannot construct stack ${err}`;
              return {hashes, stack: {state: 'hasError', error: msg}};
            }
          }
        });
      });
      return () => disposable.dispose();
    },
  ],
});

/**
 * Commit hashes being stack edited.
 * Setting to a non-empty value triggers server-side loading.
 */
export const editingStackHashes = selector({
  key: 'editingStackHashes',
  get: ({get}) => get(stackEditState).hashes,
  set: ({set}, newValue) => {
    const hashes = newValue instanceof DefaultValue ? new Set<Hash>() : newValue;
    set(stackEditState, {hashes, stack: {state: 'loading'}});
    if (hashes.size > 0) {
      const revs = getRevs(hashes);
      clientToServerAPI.postMessage({type: 'exportStack', revs});
    }
  },
});

/**
 * Main (mutable) stack state being edited.
 */
export const editingStackState = selector<Loading<CommitStackState>>({
  key: 'editingStackState',
  get: ({get}) => get(stackEditState).stack,
  set: ({set}, newValue) => {
    set(stackEditState, ({hashes, stack}) => {
      if (newValue instanceof DefaultValue) {
        // Ignore DefaultValue.
        return {hashes, stack};
      }
      if (
        stack.state === 'hasValue' &&
        newValue.state === 'hasValue' &&
        stack.value.originalStack !== newValue.value.originalStack
      ) {
        // Wrong stack. Racy edit? Ignore.
        return {hashes, stack};
      }
      return {hashes, stack: newValue};
    });
  },
});

/** Get revset expression for requested hashes. */
function getRevs(hashes: Set<Hash>): string {
  return [...hashes].join('|');
}
