/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * Takes an existing Promise and wraps it with a new Promise that will either:
 * - be fulfilled with the result from the original promise
 * - be rejected with the provided error message after `timeoutInMillis`
 *   milliseconds.
 *
 * Note that in the case where the returned Promise rejects, there is nothing
 * that stops the execution of the executor function used to create the
 * original Promise.
 */
export default function rejectAfterTimeout<T>(
  promise: Promise<T>,
  timeoutInMillis: number,
  message: string,
): Promise<T> {
  return Promise.race([
    promise,
    new Promise<T>((_resolve, reject) => {
      setTimeout(() => reject(message), timeoutInMillis);
    }),
  ]);
}
