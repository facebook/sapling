/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// Compatibility utilities

import {AbortController as AbortControllerCompat} from 'node-abort-controller';

/**
 * Like `new AbortController()` but works on older nodejs < 14.
 */
export function newAbortController(): AbortController {
  if (typeof AbortController === 'function') {
    // Prefer native AbortController.
    return new AbortController();
  } else {
    return new AbortControllerCompat() as AbortController;
  }
}
