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

test('restores the scroll position of the diff container', () => {
  const container = document.createElement('div');
  container.dataset.reviewstackDiffScroll = 'true';
  container.scrollLeft = 23;
  container.scrollTop = 1450;
  document.body.appendChild(container);

  const position = capturePullRequestScrollPosition();
  container.scrollLeft = 0;
  container.scrollTop = 0;
  restorePullRequestScrollPosition(position);

  expect(container.scrollLeft).toBe(23);
  expect(container.scrollTop).toBe(1450);
  container.remove();
});
