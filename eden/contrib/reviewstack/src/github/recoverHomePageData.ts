/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  UserHomePageQueryData,
  UserReviewRequestsQueryData,
} from '../generated/graphql';

import {GitHubGraphQLError} from './queryGraphQL';

/**
 * GitHub can return accessible pull requests alongside FORBIDDEN errors for
 * organizations that reject the token type. Preserve partial data only when
 * every error represents one unavailable repository result.
 */
function recoverRepositoryNodeData(error: unknown, pathPrefix: string[]): unknown | null {
  if (
    !(error instanceof GitHubGraphQLError) ||
    error.isRateLimitError ||
    error.errors.length === 0 ||
    !error.errors.every(({message, path, type}) => {
      const isRepositoryAccessError =
        type === 'FORBIDDEN' || message.includes('forbids access via a personal access token');
      return (
        isRepositoryAccessError &&
        path?.length === pathPrefix.length + 1 &&
        pathPrefix.every((component, index) => path[index] === component) &&
        typeof path[pathPrefix.length] === 'number'
      );
    })
  ) {
    return null;
  }
  return error.data;
}

export function recoverUserHomePageData(error: unknown): UserHomePageQueryData | null {
  const data = recoverRepositoryNodeData(error, ['viewer', 'pullRequests', 'nodes']) as
    | UserHomePageQueryData
    | null;
  return Array.isArray(data?.viewer.pullRequests.nodes) ? data : null;
}

export function recoverReviewRequestsData(error: unknown): UserReviewRequestsQueryData | null {
  const data = recoverRepositoryNodeData(error, ['search', 'nodes']) as
    | UserReviewRequestsQueryData
    | null;
  return Array.isArray(data?.search.nodes) ? data : null;
}
