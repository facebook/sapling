/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import {List} from 'immutable';
import {atom} from 'jotai';
import {tracker} from './analytics';
import {t} from './i18n';
import {atomWithOnChange, writeAtom} from './jotaiUtils';
import platform from './platform';

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
export function showToast(message: ReactNode, props?: {durationMs?: number; key?: string}) {
  const {durationMs = DEFAULT_DURATION_MS, key} = props ?? {};
  writeAtom(toastQueueAtom, oldValue => {
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
export async function copyAndShowToast(text: string, html?: string) {
  const userActivation = navigator.userActivation;
  const permissionsPolicy = (
    document as Document & {
      permissionsPolicy?: {allowsFeature: (feature: string) => boolean};
    }
  ).permissionsPolicy;
  let permissionsPolicyAllowsClipboardWrite: boolean | undefined;
  try {
    permissionsPolicyAllowsClipboardWrite = permissionsPolicy?.allowsFeature('clipboard-write');
  } catch {}
  const context = {
    documentHasFocus: document.hasFocus(),
    isRich: html != null && html !== '',
    permissionsPolicyAllowsClipboardWrite,
    userActivationHasBeenActive: userActivation?.hasBeenActive,
    userActivationIsActive: userActivation?.isActive,
    visibilityState: document.visibilityState,
  };
  try {
    if (html == null) {
      await platform.clipboardCopy(text);
    } else {
      await platform.clipboardCopy(text, html);
    }
    tracker.track('ClipboardCopy', {
      extras: {...context, outcome: 'success'},
    });
    showToast(t('Copied $text', {replace: {$text: text}}), {key: 'copied'});
  } catch (error) {
    const errorClass =
      error instanceof Error ||
      (typeof DOMException !== 'undefined' && error instanceof DOMException)
        ? error.name
        : 'UnknownError';
    tracker.track('ClipboardCopy', {
      extras: {
        ...context,
        errorClass,
        outcome:
          errorClass === 'InactiveClipboardUserActivationError'
            ? 'inactive_activation'
            : typeof DOMException !== 'undefined' && error instanceof DOMException
              ? 'native_dom_exception'
              : 'error',
      },
    });
    showToast(t('Could not copy $text', {replace: {$text: text}}), {key: 'copied'});
  }
}

/** Hide toasts with the given key. */
export function hideToast(keys: Iterable<string>) {
  const keySet = new Set(keys);
  writeAtom(toastQueueAtom, oldValue => {
    return oldValue.filter(({key}) => !keySet.has(key));
  });
}

// Private states.

type ToastProps = {
  message: ReactNode;
  key: string;
  disapparAt: Date;
};

const DEFAULT_DURATION_MS = 2000;

type ToastQueue = List<ToastProps>;

const underlyingToastQueueAtom = atom<ToastQueue>(List<ToastProps>());
export const toastQueueAtom = atomWithOnChange(underlyingToastQueueAtom, newValue => {
  const firstDisapparAt = newValue.reduce((a, t) => Math.min(a, t.disapparAt.getTime()), Infinity);
  if (firstDisapparAt === Infinity) {
    return;
  }
  const interval = Math.max(firstDisapparAt - Date.now(), 1);
  const timeout = setTimeout(() => {
    writeAtom(underlyingToastQueueAtom, oldValue => removeExpired(oldValue));
  }, interval);
  return () => clearTimeout(timeout);
});

function removeExpired(queue: ToastQueue) {
  const now = new Date();
  const newQueue = queue.filter(({disapparAt}) => disapparAt > now);
  return newQueue.size < queue.size ? newQueue : queue;
}
