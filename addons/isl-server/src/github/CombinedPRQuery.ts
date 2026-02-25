/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  Exact,
  PullRequestReviewDecision,
  PullRequestState,
  Scalars,
  StatusState,
} from './generated/graphql';

// PR node shape shared by both open and closed search results.
export type PRNode = {
  __typename: 'PullRequest';
  number: number;
  title: string;
  body: string;
  state: PullRequestState;
  isDraft: boolean;
  url: string;
  reviewDecision?: PullRequestReviewDecision | null;
  author?: {login: string; avatarUrl: string} | null;
  comments: {totalCount: number};
  baseRef?: {
    name: string;
    target?: {oid: string} | null;
  } | null;
  headRef?: {
    name: string;
    target?: {oid: string} | null;
  } | null;
  commits: {
    nodes?: Array<{
      commit: {
        statusCheckRollup?: {
          state: StatusState;
        } | null;
      };
    } | null> | null;
  };
};

type SearchNode =
  | {__typename?: 'App'}
  | {__typename?: 'Discussion'}
  | {__typename?: 'Issue'}
  | {__typename?: 'MarketplaceListing'}
  | {__typename?: 'Organization'}
  | PRNode
  | {__typename?: 'Repository'}
  | {__typename?: 'User'}
  | null;

export type CombinedPRQueryVariables = Exact<{
  openQuery: Scalars['String'];
  closedQuery: Scalars['String'];
  numToFetch: Scalars['Int'];
}>;

export type CombinedPRQueryData = {
  viewer: {login: string};
  open: {nodes?: Array<SearchNode> | null};
  closed: {nodes?: Array<SearchNode> | null};
};

// Combined query fetches open + closed PRs in a single GraphQL request.
// Expensive fields (mergeable, mergeStateStatus, viewerCanMergeAsAdmin, CI check
// contexts) are omitted to avoid GitHub 502 timeouts on large result sets.
// Those fields are only needed in merge/review mode and can be lazy-loaded.
export const CombinedPRQuery = `
  query CombinedPRQuery($openQuery: String!, $closedQuery: String!, $numToFetch: Int!) {
    viewer {
      login
    }
    open: search(query: $openQuery, type: ISSUE, first: $numToFetch) {
      nodes {
        ... on PullRequest {
          __typename
          number
          title
          body
          state
          isDraft
          author {
            login
            avatarUrl
          }
          url
          reviewDecision
          comments {
            totalCount
          }
          baseRef {
            target {
              oid
            }
            name
          }
          headRef {
            target {
              oid
            }
            name
          }
          commits(last: 1) {
            nodes {
              commit {
                statusCheckRollup {
                  state
                }
              }
            }
          }
        }
      }
    }
    closed: search(query: $closedQuery, type: ISSUE, first: $numToFetch) {
      nodes {
        ... on PullRequest {
          __typename
          number
          title
          body
          state
          isDraft
          author {
            login
            avatarUrl
          }
          url
          reviewDecision
          comments {
            totalCount
          }
          baseRef {
            target {
              oid
            }
            name
          }
          headRef {
            target {
              oid
            }
            name
          }
          commits(last: 1) {
            nodes {
              commit {
                statusCheckRollup {
                  state
                }
              }
            }
          }
        }
      }
    }
  }
`;
