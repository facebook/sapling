/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * A token which represents some ongoing work which may be cancelled.
 * Typically created by the caller of some async work,
 * and used by the caller to cancel and used by the
 * implementation to observe and respond to cancellations.
 * This is similar to CancellationToken used in VS Code.
 *
 * Tokens should only be cancelled once.
 *
 * Token can be polled with isCancelled,
 * or you can subscribe with onCancel.
 */
export class CancellationToken {
  public isCancelled = false;

  private callbacks: Array<() => unknown> = [];
  public onCancel(cb: () => unknown) {
    if (this.isCancelled) {
      cb();
      return () => undefined;
    }
    this.callbacks.push(cb);
    return () => {
      const position = this.callbacks.indexOf(cb);
      if (position !== -1) {
        this.callbacks.splice(position, 1);
      }
    };
  }

  public cancel() {
    if (!this.isCancelled) {
      this.isCancelled = true;
      this.callbacks.forEach(c => c());
    }
  }
}
