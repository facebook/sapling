/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Tracker} from 'isl-server/src/analytics/tracker';

declare global {
  interface Window {
    globalIslClientTracker: Tracker<Record<string, never>>;
  }
}
/**
 * Globally access analytics tracker, to prevent cyclical imports.
 * Should technically only be nullable if used at the top level.
 */
export function getTracker(): Tracker<Record<string, never>> | undefined {
  return window.globalIslClientTracker;
}
