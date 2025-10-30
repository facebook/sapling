/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {useAtomValue} from 'jotai';
import {localStorageBackedAtom, readAtom, writeAtom} from '../jotaiUtils';
import type {ActionMenuItem} from './types';

/**
 * Used to keep track of smart actions by order of use.
 * Use {@link bumpSmartAction} to add/update an action to the cache.
 * Do not modify the cache directly.
 */
const smartActionsOrder = localStorageBackedAtom<Array<string>>('isl.smart-actions-order', []);

/**
 * Given an array of smart actions, returns the same actions sorted by usage.
 */
export function useSortedActions(actions: Array<ActionMenuItem>) {
  const cache = useAtomValue(smartActionsOrder);
  return [...actions].sort((a, b) => {
    const aIndex = cache.indexOf(a.id) >= 0 ? cache.indexOf(a.id) : Infinity;
    const bIndex = cache.indexOf(b.id) >= 0 ? cache.indexOf(b.id) : Infinity;
    return aIndex - bIndex;
  });
}

/**
 * Marks an action as used, updating the cache.
 */
export function bumpSmartAction(action: string) {
  const cache = readAtom(smartActionsOrder);
  const index = cache.indexOf(action);
  let newCache = [...cache];
  // Remove the action if it's already in the cache
  if (index !== -1) {
    newCache.splice(index, 1);
  }
  // For now, use LRU ordering
  // TODO: Consider using frecency ordering
  newCache = [action, ...newCache];
  writeAtom(smartActionsOrder, newCache);
}
