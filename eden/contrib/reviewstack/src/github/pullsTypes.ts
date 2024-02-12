/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PageInfo, PullRequestState, PullsPullRequestFragment} from '../generated/graphql';
import type {PaginationParams} from './types';

export type PullsQueryInput = PaginationParams & {
  labels: string[];
  states: PullRequestState[];
};

export type PullsPullRequest = PullsPullRequestFragment;

export type PullsWithPageInfo = {
  pullRequests: PullsPullRequest[];
  pageInfo: PageInfo;
  totalCount: number;
};
