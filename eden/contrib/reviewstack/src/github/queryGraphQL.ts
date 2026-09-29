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
  readonly isRateLimitError: boolean;
  readonly rateLimitReset: number | null;

  constructor(
    readonly errors: GraphQLResponseError[],
    readonly data: unknown,
    responseHeaders?: Headers,
  ) {
    super(
      errors
        .map(error => {
          const path = Array.isArray(error.path) ? ` (${error.path.join('.')})` : '';
          return `${error.message}${path}`;
        })
        .join('\n'),
    );
    this.name = 'GitHubGraphQLError';
    this.isRateLimitError = errors.some(
      error => error.type === 'RATE_LIMIT' || error.message.includes('rate limit'),
    );
    const rateLimitReset = Number(responseHeaders?.get('x-ratelimit-reset'));
    this.rateLimitReset =
      Number.isFinite(rateLimitReset) && rateLimitReset > 0 ? rateLimitReset : null;
  }
}

export class GitHubNetworkError extends Error {
  constructor(readonly originalError: unknown) {
    super('ReviewStack could not reach GitHub. Check your connection and try again.');
    this.name = 'GitHubNetworkError';
  }
}

const NETWORK_RETRY_DELAYS_MS = [250, 1000];

async function fetchGraphQL(
  graphQLEndpoint: string,
  requestHeaders: Record<string, string>,
  body: string,
  retryNetworkErrors: boolean,
  retryDelays: number[] = NETWORK_RETRY_DELAYS_MS,
): Promise<Response> {
  try {
    return await fetch(graphQLEndpoint, {
      headers: requestHeaders,
      method: 'POST',
      body,
    });
  } catch (error) {
    const [delay, ...remainingDelays] = retryDelays;
    if (!retryNetworkErrors || delay == null) {
      throw new GitHubNetworkError(error);
    }
    await new Promise(resolve => window.setTimeout(resolve, delay));
    return fetchGraphQL(
      graphQLEndpoint,
      requestHeaders,
      body,
      retryNetworkErrors,
      remainingDelays,
    );
  }
}

export default async function queryGraphQL<TData, TVariables>(
  query: string,
  variables: TVariables,
  requestHeaders: Record<string, string>,
  graphQLEndpoint: string,
): Promise<TData> {
  // A rejected fetch has no HTTP response and is usually a transient browser,
  // DNS, or CORS-path failure. Queries are safe to retry; mutations are not.
  const response = await fetchGraphQL(
    graphQLEndpoint,
    requestHeaders,
    JSON.stringify({query, variables}),
    query.trimStart().startsWith('query '),
  );

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
    throw new GitHubGraphQLError(json.errors, json.data, response.headers);
  }

  return json.data;
}
