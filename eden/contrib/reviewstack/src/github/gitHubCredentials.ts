/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * Creates the GraphQL endpoint URL for a given hostname.
 *
 * According to GitHub's documentation:
 * https://docs.github.com/en/enterprise-server@3.6/graphql/guides/introduction-to-graphql#discovering-the-graphql-api
 *
 * The URL to use for the GraphQL API is:
 *   http(s)://HOSTNAME/api/graphql
 *
 * Though for consumer GitHub, the endpoint is:
 *   https://api.github.com/graphql
 */
export function createGraphQLEndpointForHostname(hostname: string): string {
  if (hostname === 'github.com') {
    return 'https://api.github.com/graphql';
  } else {
    return `https://${hostname}/api/graphql`;
  }
}
