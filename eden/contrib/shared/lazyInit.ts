/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// See https://advancedweb.hu/the-async-lazy-initializer-pattern-in-javascript/

/**
 * Because Promises are eager in JavaScript, we need to introduce an extra layer
 * to lazily invoke an async operation. lazyInit() takes a function that
 * represents the async operation, but does not call it until the function
 * returned by lazyInit() itself is called. Note that lazyInit() is idempotent:
 * once it is called, it will always return the original Promise created by
 * calling the async operation.
 *
 * ```
 * // Note getObj is a *function*, not a *Promise*.
 * const getObj = lazyInit(async () => {
 *   const value = await expensiveOperation();
 *   return value + 1;
 * });
 *
 * ...
 *
 * // expensiveObjCreation() will not be called until getObj() is called, and if
 * // it is called, it will only be called once.
 * const objRef1 = await getObj();
 * const objRef2 = await getObj();
 * ```
 */
export default function lazyInit<T>(init: () => Promise<T>): () => Promise<T> {
  let promise: Promise<T> | null = null;
  return () => (promise = promise ?? init());
}
