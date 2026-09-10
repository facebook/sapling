/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import UnauthorizedError from './UnauthorizedError';

type GraphQLResponseError = {
  message: string;
  type?: string;
  path?: Array<string | number>;
};

export class GitHubGraphQLError extends Error {
  constructor(readonly errors: GraphQLResponseError[], readonly data: unknown) {
    super(
      errors
        .map(error => {
          const path = Array.isArray(error.path) ? ` (${error.path.join('.')})` : '';
          return `${error.message}${path}`;
        })
        .join('\n'),
    );
    this.name = 'GitHubGraphQLError';
  }
}

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

  if (Array.isArray(json.errors) && json.errors.length > 0) {
    throw new GitHubGraphQLError(json.errors, json.data);
  }

  return json.data;
}
