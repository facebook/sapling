/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  CheckConclusionState,
  CheckStatusState,
  Exact,
  MergeableState,
  MergeStateStatus,
  PullRequestReviewDecision,
  PullRequestState,
  Scalars,
  StatusState,
} from './generated/graphql';

// PR node shape shared by both open and closed search results.
// Includes mergeQueueEntry optionally (null when merge queues not supported).
type PRNode = {
  __typename: 'PullRequest';
  number: number;
  title: string;
  body: string;
  state: PullRequestState;
  isDraft: boolean;
  url: string;
  reviewDecision?: PullRequestReviewDecision | null;
  mergeable: MergeableState;
  mergeStateStatus: MergeStateStatus;
  viewerCanMergeAsAdmin: boolean;
  author?: {login: string; avatarUrl: string} | null;
  comments: {totalCount: number};
  mergeQueueEntry?: {estimatedTimeToMerge?: number | null} | null;
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
        oid: string;
        statusCheckRollup?: {
          state: StatusState;
          contexts: {
            nodes?: Array<
              | {
                  __typename: 'CheckRun';
                  name: string;
                  status: CheckStatusState;
                  conclusion?: CheckConclusionState | null;
                  detailsUrl?: string | null;
                }
              | {
                  __typename: 'StatusContext';
                  context: string;
                  state: StatusState;
                  targetUrl?: string | null;
                }
              | null
            > | null;
          };
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
  __type?: {__typename: '__Type'} | null;
  viewer: {login: string};
  open: {nodes?: Array<SearchNode> | null};
  closed: {nodes?: Array<SearchNode> | null};
};

const PR_FIELDS = `
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
        mergeable
        mergeStateStatus
        viewerCanMergeAsAdmin
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
              oid
              statusCheckRollup {
                state
                contexts(first: 100) {
                  nodes {
                    __typename
                    ... on CheckRun {
                      name
                      status
                      conclusion
                      detailsUrl
                    }
                    ... on StatusContext {
                      context
                      state
                      targetUrl
                    }
                  }
                }
              }
            }
          }
        }`;

const MERGE_QUEUE_FIELDS = `
        mergeQueueEntry {
          estimatedTimeToMerge
        }`;

function buildCombinedQuery(includeMergeQueue: boolean): string {
  const mqFields = includeMergeQueue ? MERGE_QUEUE_FIELDS : '';
  return `
    query CombinedPRQuery($openQuery: String!, $closedQuery: String!, $numToFetch: Int!) {
  __type(name: "MergeQueueEntry") {
    __typename
  }
  viewer {
    login
  }
  open: search(query: $openQuery, type: ISSUE, first: $numToFetch) {
    nodes {
      ... on PullRequest {
${PR_FIELDS}${mqFields}
      }
    }
  }
  closed: search(query: $closedQuery, type: ISSUE, first: $numToFetch) {
    nodes {
      ... on PullRequest {
${PR_FIELDS}${mqFields}
      }
    }
  }
}
    `;
}

export const CombinedPRQueryWithMergeQueue = buildCombinedQuery(true);
export const CombinedPRQueryWithoutMergeQueue = buildCombinedQuery(false);
