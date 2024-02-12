/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {useEffect, useMemo} from 'react';

function debounce<T>(
  f: (arg: T) => void,
  wait: number,
): {
  debounced: (arg: T) => void;
  reset: () => void;
} {
  let timeout: number | undefined;

  function reset() {
    window.clearTimeout(timeout);
    timeout = undefined;
  }

  function debounced(arg: T): void {
    reset();
    timeout = window.setTimeout(() => {
      reset();
      f(arg);
    }, wait);
  }

  return {
    debounced,
    reset,
  };
}

export default function useDebounced<T>(f: (arg: T) => void, wait = 500): (arg: T) => void {
  const {debounced, reset} = useMemo(() => debounce(f, wait), [f, wait]);

  useEffect(() => reset, [reset]);

  return debounced;
}
