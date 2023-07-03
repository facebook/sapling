/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * Return the promise result (either 'ok' or 'error'), or "suspend" React's rendering.
 * The callsite should have `<Suspense>` in a parent component to support suspension.
 *
 * Note this function translates error to a regular value without throwing.
 * This avoids the need of another "ErrorBoundary". The callsite could rethrow
 * if it needs to work with an "ErrorBoundary".
 *
 * Be aware that the `promise` should not be created inside `<Suspense>`, because
 * those components do not have states if `<Suspense>` renders `fallback`. Instead,
 * the `promise` should be passed from the parent component that renders `<Suspense>`.
 *
 * Example:
 *
 * ```
 * // Parent component with `<Suspense>`.
 * function Container(props: {path: string}) {
 *   const loader = useLoader();
 *   const promise = loader.load(props.path);
 *   return <Suspense fallback={<Loading/>}><Inner promise={promise} /></Suspense>
 * }
 *
 * // Child component using `usePromise`.
 * function Inner(props: {promise: ...}) {
 *   const [status, data] = usePromise(props.promise);
 *   if (status === 'error') {
 *     return <ErrorMessage error={data} />;
 *   } else {
 *     return <Data data={data} />;
 *   }
 * }
 * ```
 *
 * See also https://github.com/reactjs/react.dev/blob/3364c93feb358a7d1ac2e8d8b0468c3e32214062/src/content/reference/react/Suspense.md?plain=1#L141
 */
export function usePromise<T>(promise: PromiseExt<T>): ['ok', T] | ['error', Error] {
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
  } else {
    return status;
  }
}

export interface PromiseExt<T> extends Promise<T> {
  usePromiseStatus?: ['ok', T] | ['error', Error] | 'pending';
}
