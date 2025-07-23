/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Disposable} from './types';

export class Timer implements Disposable {
  private timerId: null | number = null;
  private disposed = false;
  private callback: () => void;

  /**
   * The `callback` can return `false` to auto-stop the timer.
   * The timer auto stops if being GC-ed.
   */
  constructor(
    callback: () => void | boolean,
    public intervalMs = 1000,
    enabled = false,
  ) {
    const thisRef = new WeakRef(this);
    this.callback = () => {
      const timer = thisRef.deref();
      if (timer == null) {
        // The "timer" object is GC-ed. Do not run this interval.
        return;
      }
      // Run the callback and schedules the next interval.
      timer.timerId = null;
      const shouldContinue = callback();
      if (shouldContinue !== false) {
        timer.enabled = true;
      }
    };
    this.enabled = enabled;
  }

  set enabled(value: boolean) {
    if (value && this.timerId === null && !this.disposed) {
      this.timerId = window.setTimeout(this.callback, this.intervalMs);
    } else if (!value && this.timerId !== null) {
      window.clearTimeout(this.timerId);
      this.timerId = null;
    }
  }

  get enabled(): boolean {
    return this.timerId !== null;
  }

  dispose(): void {
    this.enabled = false;
    this.disposed = true;
  }
}
