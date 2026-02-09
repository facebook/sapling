/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import UnauthorizedError from './UnauthorizedError';

export default async function queryGraphQL<TData, TVariables>(
  query: string,
  variables: TVariables,
  requestHeaders: Record<string, string>,
  graphQLEndpoint: string,
): Promise<TData> {
  const response = await fetch(graphQLEndpoint, {
    headers: requestHeaders,
    method: 'POST',
    body: JSON.stringify({query, variables}),
  });

  if (!response.ok) {
    if (response.status === 401) {
      throw new UnauthorizedError(
        'Your GitHub access token has expired or been revoked. Please sign in again.',
      );
    }
    return Promise.reject(`HTTP request error: ${response.status}: ${response.statusText}`);
  }

  const json = await response.json();

  if (Array.isArray(json.errors)) {
    return Promise.reject(`Error: ${json.errors[0].message}`);
  }

  return json.data;
}
