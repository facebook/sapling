/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {countPullRequestComments} from './pullRequestCommentCount';

test('counts resolved review thread comments in the stack total', () => {
  const count = countPullRequestComments({
    comments: {totalCount: 2},
    reviews: {
      nodes: [{body: 'Approved'}, {body: ''}, null],
    },
    reviewThreads: {
      nodes: [{comments: {totalCount: 3}}, {comments: {totalCount: 2}}, null],
    },
  });

  expect(count).toBe(8);
});
