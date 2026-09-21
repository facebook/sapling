/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {
  capturePullRequestScrollPosition,
  restorePullRequestScrollPosition,
} from './pullRequestScroll';

test('restores the diff and timeline positions after a comment refresh', () => {
  const diffContainer = document.createElement('div');
  diffContainer.dataset.reviewstackDiffScroll = 'true';
  diffContainer.scrollLeft = 23;
  diffContainer.scrollTop = 1450;
  document.body.appendChild(diffContainer);
  const timelineContainer = document.createElement('div');
  timelineContainer.dataset.reviewstackTimelineScroll = 'true';
  timelineContainer.scrollTop = 820;
  document.body.appendChild(timelineContainer);

  const position = capturePullRequestScrollPosition();
  diffContainer.scrollLeft = 0;
  diffContainer.scrollTop = 0;
  timelineContainer.scrollTop = 0;
  restorePullRequestScrollPosition(position);

  expect(diffContainer.scrollLeft).toBe(23);
  expect(diffContainer.scrollTop).toBe(1450);
  expect(timelineContainer.scrollTop).toBe(820);
  diffContainer.remove();
  timelineContainer.remove();
});
