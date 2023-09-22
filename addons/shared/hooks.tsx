/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {debounce} from './debounce';
import deepEqual from 'fast-deep-equal';
import {useCallback, useEffect, useMemo, useRef} from 'react';

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
  const throttled = useCallback(debounce(cb, throttleTimeMs, undefined, true), [
    throttleTimeMs,
    ...(deps ?? []),
  ]);
  return useEffect((...args: A) => {
    return throttled(...args);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
}

/**
 * Like React.useMemo, but with deep equality comparison between previous/next dependencies.
 */
export function useDeepMemo<T>(construct: () => T, dependencies: React.DependencyList) {
  const ref = useRef<React.DependencyList>([]);
  if (!deepEqual(dependencies, ref.current)) {
    ref.current = dependencies;
  }
  const deepDeps = ref.current;

  // eslint-disable-next-line react-hooks/exhaustive-deps
  return useMemo(construct, deepDeps);
}

/**
 * Returns a react ref that you can pass to an element to autofocus it on mount.
 */
export function useAutofocusRef(): React.MutableRefObject<HTMLElement | null> {
  const ref = useRef<HTMLElement | null>(null);
  useEffect(() => {
    if (ref.current != null) {
      ref.current.focus();
    }
  }, [ref]);
  return ref;
}
