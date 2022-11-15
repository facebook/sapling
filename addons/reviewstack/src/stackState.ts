/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {StackPullRequestFragment} from './generated/graphql';
import type {SaplingPullRequestBody} from './saplingStack';

import {pullRequestNumbersFromBody} from './ghstackUtils';
import {gitHubClient, gitHubPullRequest} from './recoil';
import {parseSaplingStackBody} from './saplingStack';
import {selector, waitForAll} from 'recoil';

type StackedPullRequest =
  | {
      type: 'sapling';
      body: SaplingPullRequestBody;
    }
  | {
      type: 'ghstack';
      stack: number[];
    }
  | {
      type: 'no-stack';
    };

export const stackedPullRequest = selector<StackedPullRequest>({
  key: 'stackedPullRequest',
  get: ({get}) => {
    const pullRequest = get(gitHubPullRequest);
    const body = pullRequest?.body;
    if (body != null) {
      const saplingStack = parseSaplingStackBody(body);
      if (saplingStack != null) {
        return {type: 'sapling', body: saplingStack};
      }

      const ghstack = pullRequestNumbersFromBody(body);
      if (ghstack != null) {
        return {type: 'ghstack', stack: ghstack};
      }
    }

    return {type: 'no-stack'};
  },
});

const stackedPullRequestNumbers = selector<number[]>({
  key: 'stackedPullRequestNumbers',
  get: ({get}) => {
    const stacked = get(stackedPullRequest);
    switch (stacked.type) {
      case 'no-stack':
        return [];
      case 'sapling': {
        return stacked.body.stack.map(({number}) => number);
      }
      case 'ghstack': {
        return stacked.stack;
      }
    }
  },
});

export const stackedPullRequestFragments = selector<StackPullRequestFragment[]>({
  key: 'stackedPullRequestFragments',
  get: ({get}) => {
    const [client, prs] = get(waitForAll([gitHubClient, stackedPullRequestNumbers]));
    if (client == null) {
      return [];
    }
    return client.getStackPullRequests(prs);
  },
});
