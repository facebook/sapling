/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {isPromise} from 'shared/utils';

/**
 * Return the promise result, raise the promise error, or "suspend" React's rendering.
 * The callsite should have `<Suspense>` and `<ErrorBoundary>` in a parent component
 * to support suspension and error handling, or use `<SuspenseBoundary>`.
 *
 * Be aware that the function that produces `promise` should returning
 * `Promise.resolve(data)`, and instead return `data` directly. This is because
 * Javascript does not provide a way to test if a Promise is resolved without async.
 * So the Promise will be treated as "pending" temporarily, rendering the Suspense
 * fallback. The actual Suspense children will lose their states because the
 * fallback replaces them. See the `maybePromise` below.
 *
 * Example:
 *
 * ```
 * // Parent component with `<Suspense>`.
 * function Container(props: {path: string}) {
 *   return <SuspenseBoundary><Inner /></SuspenseBoundary>;
 * }
 *
 * // Child component using `usePromise`.
 * function Inner() {
 *   const data = usePromise(maybePromise());
 *   return <Data data={data} />;
 * }
 *
 * function maybePromise(): Data | Promise<Data> {
 *   if (isDataReady()) {
 *     // Do not return Promise.resolve(data). That loses <Inner /> state.
 *     return data;
 *   }
 *   ...
 * }
 * ```
 *
 * Alternatively, the promise can be passed from a more "stable" stateful
 * parent component that keeps the promise object unchanged when <Suspense>
 * switches between fallback to non-fallback:
 *
 * ```
 * // Parent component with `<Suspense>`.
 * function Container(props: {path: string}) {
 *   const loader = useLoader();
 *   const promise = loader.load(props.path);
 *   return <SuspenseBoundary><Inner promise={promise} /></SuspenseBoundary>;
 * }
 *
 * // Child component using `usePromise`.
 * function Inner(props: {promise: ...}) {
 *   // The promise is from the parent of <Suspense />.
 *   const data = usePromise(promise);
 *   return <Data data={data} />;
 * }
 * ```
 *
 * See also https://github.com/reactjs/react.dev/blob/3364c93feb358a7d1ac2e8d8b0468c3e32214062/src/content/reference/react/Suspense.md?plain=1#L141
 */
export function usePromise<T>(promise: T | PromiseExt<T>): T {
  if (!isPromise(promise)) {
    return promise;
  }
  const status = promise.usePromiseStatus;
  if (status === undefined) {
    promise.usePromiseStatus = 'pending';
    promise.then(
      resolve => {
        promise.usePromiseStatus = ['ok', resolve];
      },
      error => {
        promise.usePromiseStatus = ['error', error];
      },
    );
    // This is the undocumented API to make <Suspense /> render its fallback.
    // React might change it in the future. But it has been like this for years.
    throw promise;
  } else if (status === 'pending') {
    throw promise;
  } else if (status[0] === 'ok') {
    return status[1];
  } else {
    throw status[1];
  }
}

export interface PromiseExt<T> extends Promise<T> {
  usePromiseStatus?: ['ok', T] | ['error', Error] | 'pending';
}
