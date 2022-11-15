/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export type DebouncedFunction<Args extends Array<unknown>> = {
  (...args: Args): void;
  reset: () => void;
  isPending: () => boolean;
};

/**
 * This is a rate limiting mechanism, used to invoke a function after a repeated
 * action has completed.  This creates and returns a debounced version of
 * the function passed in that will postpone its execution until after `wait`
 * milliseconds have elapsed since the last time it was invoked.
 *
 * For example, if you wanted to update a preview after the user stops typing
 * you could do the following:
 *
 *   elem.addEventListener('keyup', debounce(this.updatePreview, 250), false);
 *
 * The returned function has a reset method which can be called to cancel a
 * pending invocation.
 *
 *   var debouncedUpdatePreview = debounce(this.updatePreview, 250);
 *   elem.addEventListener('keyup', debouncedUpdatePreview, false);
 *
 *   // later, to cancel pending calls
 *   debouncedUpdatePreview.reset();
 *
 * @param func - the function to debounce
 * @param wait - how long to wait in milliseconds
 * @param context - optional context to invoke the function in
 * @param leading - cause debounce to trigger the function on
 *  the leading edge instead of the trailing edge of the wait interval
 */
export function debounce<Args extends Array<unknown>>(
  func: (...args: Args) => unknown,
  wait: number,
  context: unknown = undefined,
  leading = false,
): DebouncedFunction<Args> {
  let timeout: NodeJS.Timeout | undefined;
  let shouldCallLeading = true;

  function debouncer(...args: Args) {
    let callback: () => void;

    if (leading) {
      callback = function () {
        shouldCallLeading = true;
        timeout = undefined;
      };

      if (!shouldCallLeading) {
        clearTimeout(timeout);
        timeout = setTimeout(callback, wait);
        return;
      }

      shouldCallLeading = false;
      func.apply(context, args);
    } else {
      debouncer.reset();
      callback = function () {
        timeout = undefined;
        func.apply(context, args);
      };
    }

    timeout = setTimeout(callback, wait);
  }

  debouncer.reset = function () {
    clearTimeout(timeout);
    timeout = undefined;
    shouldCallLeading = true;
  };

  debouncer.isPending = function () {
    return timeout != null;
  };

  return debouncer;
}
