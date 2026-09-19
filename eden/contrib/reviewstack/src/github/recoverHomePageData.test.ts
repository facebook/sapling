/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {GitHubGraphQLError} from './queryGraphQL';
import {
  recoverReviewRequestsData,
  recoverUserHomePageData,
} from './recoverHomePageData';

const reviewRequestsData = {
  search: {
    nodes: [null],
  },
};

const homePageData = {
  viewer: {
    pullRequests: {
      nodes: [null],
    },
  },
};

test('keeps authored PRs while omitting inaccessible repository nodes', () => {
  const error = new GitHubGraphQLError(
    [
      {
        message:
          '`RestrictedOrg` forbids access via a personal access token (classic). Please use another token type.',
        path: ['viewer', 'pullRequests', 'nodes', 46],
      },
      {
        message:
          '`RestrictedOrg` forbids access via a personal access token (classic). Please use another token type.',
        path: ['viewer', 'pullRequests', 'nodes', 49],
      },
    ],
    homePageData,
  );

  expect(recoverUserHomePageData(error)).toBe(homePageData);
});

test('keeps review requests while omitting inaccessible repository nodes', () => {
  const error = new GitHubGraphQLError(
    [
      {
        message:
          '`RestrictedOrg` forbids access via a personal access token (classic). Please use another token type.',
        path: ['search', 'nodes', 19],
      },
    ],
    reviewRequestsData,
  );

  expect(recoverReviewRequestsData(error)).toBe(reviewRequestsData);
});

test('keeps partial data for explicit forbidden node errors', () => {
  const error = new GitHubGraphQLError(
    [{message: 'Resource not accessible', type: 'FORBIDDEN', path: ['search', 'nodes', 4]}],
    reviewRequestsData,
  );

  expect(recoverReviewRequestsData(error)).toBe(reviewRequestsData);
});

test.each([
  new Error('network failed'),
  new GitHubGraphQLError(
    [{message: 'Resource not accessible', type: 'FORBIDDEN', path: ['viewer', 'login']}],
    homePageData,
  ),
  new GitHubGraphQLError(
    [{message: 'Something failed', path: ['viewer', 'pullRequests', 'nodes', 46]}],
    homePageData,
  ),
  new GitHubGraphQLError(
    [
      {
        message: 'API rate limit exceeded',
        type: 'RATE_LIMIT',
        path: ['viewer', 'pullRequests', 'nodes', 46],
      },
    ],
    homePageData,
  ),
  new GitHubGraphQLError(
    [
      {
        message: 'Resource not accessible',
        type: 'FORBIDDEN',
        path: ['viewer', 'pullRequests', 'nodes', 46],
      },
    ],
    null,
  ),
])('does not hide unrelated or unusable errors', error => {
  expect(recoverUserHomePageData(error)).toBeNull();
});
