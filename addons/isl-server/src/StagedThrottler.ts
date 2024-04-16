/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {nullthrows} from 'shared/utils';

/**
 * Incrementally increases throttling when an even starts happening too often.
 * For example, initially there's no throttle
 * After 10 events without a gap of 10s, there's a 10s throttle.
 * After 30 events without a gap of 30s, there's a 30s throttle.
 * After no events for 10s, the throttle is reset to 0.
 *
 * These thresholds are configurable.
 * "Throttling" means dropping events after the first one (unlike debouncing).
 */
export function stagedThrottler<P extends Array<unknown>>(
  stages: Array<{
    throttleMs: number;
    /** number of input events needed to advance to the enxt stage.
     * Note: it doesn't matter if it was throttled or not. Every input adds to the advancement. */
    numToNextStage?: number;
    resetAfterMs: number;
    /** Called when entering a stage.
     * Note: 0th stage onEnter is not called "on startup", only if you reset the stage,
     * and that this stage resets the next time a value IS emitted, not merely once the time passes.
     */
    onEnter?: () => unknown;
  }>,
  cb: (...args: P) => void,
) {
  // Time of the last non-throttled call
  let lastEmitted = -Infinity;
  let currentStage = 0;
  let numSeen = 0;

  return (...args: P) => {
    const stage = nullthrows(stages[currentStage]);
    const currentThrottle = stage.throttleMs;
    const elapsed = Date.now() - lastEmitted;

    // Input always counts towards going to the next stage
    numSeen++;

    // Maybe go to the next stage
    if (numSeen > 1 && elapsed > stage.resetAfterMs) {
      // Reset the throttle
      numSeen = 0;
      currentStage = 0;
      stages[currentStage].onEnter?.();
    } else if (stage.numToNextStage && numSeen >= stage.numToNextStage) {
      const nextStage = currentStage + 1;
      if (nextStage < stages.length) {
        numSeen = 0;
        currentStage++;
        stages[currentStage].onEnter?.();
      }
    }

    if (elapsed < currentThrottle) {
      // Needs to be throttled
      return;
    }

    // No need to throttle
    lastEmitted = Date.now();
    return cb(...args);
  };
}
