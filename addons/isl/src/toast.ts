/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';
import type {SetterOrUpdater} from 'recoil';

import {t} from './i18n';
import platform from './platform';
import {List} from 'immutable';
import {DefaultValue, atom, useRecoilState} from 'recoil';

export function useToast(): UseToast {
  const [queue, setQueue] = useRecoilState(toastQueueAtom);
  return new UseToast(queue, setQueue);
}

/** Features related to toasts. */
class UseToast {
  constructor(private queue: ToastQueue, private setQueue: SetterOrUpdater<ToastQueue>) {}

  /**
   * Push a toast. It will be displayed immediately and auto hides after a timeout.
   *
   * If `key` is specified, an existing toast with the same key will be replaced.
   * This can be useful to ensure there are no 2 "Copied <text>" toasts at the same time,
   * since the clipboard can only hold a single value.
   *
   * Note the internals use O(N) scans in various places.
   * Do not push too many toasts.
   */
  show(message: ReactNode, props?: {durationMs?: number; key?: string}) {
    const {durationMs = DEFAULT_DURATION_MS, key} = props ?? {};
    this.setQueue(oldValue => {
      let nextValue = oldValue;
      const hideAt = new Date(Date.now() + durationMs);
      if (key != null) {
        // Remove an existing toast with the same key.
        nextValue = nextValue.filter(({key: k}) => k !== key);
      }
      return nextValue.push({message, disapparAt: hideAt, key: key ?? hideAt.getTime().toString()});
    });
  }

  /** Show "Copied <text>" toast. Existing "copied' toast will be replaced. */
  copyAndShowToast(text: string) {
    platform.clipboardCopy(text);
    this.show(t('Copied $text', {replace: {$text: text}}), {key: 'copied'});
  }

  /** List all toasts to display. */
  list(): List<ToastProps> {
    return this.queue;
  }

  /** Hide toasts with the given key. */
  hide(keys: Iterable<string>) {
    const keySet = new Set(keys);
    this.setQueue(oldValue => {
      return oldValue.filter(({key}) => !keySet.has(key));
    });
  }
}

// Private states.

type ToastProps = {
  message: ReactNode;
  key: string;
  disapparAt: Date;
};

const DEFAULT_DURATION_MS = 2000;

type ToastQueue = List<ToastProps>;

const toastQueueAtom = atom<ToastQueue>({
  key: 'toastQueueAtom',
  default: List<ToastProps>(),
  effects: [
    // Clean up expired toasts after the first `disapparAt`.
    ({onSet, setSelf}) => {
      onSet(newValue => {
        if (newValue instanceof DefaultValue) {
          return;
        }
        const firstDisapparAt = newValue.reduce(
          (a, t) => Math.min(a, t.disapparAt.getTime()),
          Infinity,
        );
        if (firstDisapparAt === Infinity) {
          return;
        }
        const interval = Math.max(firstDisapparAt - Date.now(), 1);
        const timeout = setTimeout(() => {
          setSelf(oldValue =>
            oldValue instanceof DefaultValue ? oldValue : removeExpired(oldValue),
          );
        }, interval);
        return () => clearTimeout(timeout);
      });
    },
  ],
});

function removeExpired(queue: ToastQueue) {
  const now = new Date();
  const newQueue = queue.filter(({disapparAt}) => disapparAt > now);
  return newQueue.size < queue.size ? newQueue : queue;
}
