/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {debounce} from './debounce';
import {useCallback, useEffect} from 'react';

/**
 * Like useEffect, but throttles calls to the effect callback.
 * This can help avoid overfiring effects that need to happen during render.
 *
 * Note: Do not use this just to bypass effects firing twice
 * in strict + dev mode. Double-firing is done to help detect bugs.
 * Throttling is not suitable for subscriptions that must stay in sync
 * or queries which need to stay in sync as things update.
 *
 * This is most useful for best-effort side-effects like logging & analytics
 * which don't require exact synchronization and don't affect UI state.
 */
export function useThrottledEffect<A extends Array<unknown>>(
  cb: (...args: A) => void,
  throttleTimeMs: number,
  deps?: Array<unknown>,
): void {
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const throttled = useCallback(debounce(cb, throttleTimeMs, undefined, true), [throttleTimeMs]);
  return useEffect((...args: A) => {
    return throttled(...args);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
}
