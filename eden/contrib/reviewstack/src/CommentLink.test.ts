/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {commentAnchorID, commentPermalink} from './commentLinkUtils';

describe('comment links', () => {
  test('creates stable timeline and diff anchors', () => {
    expect(commentAnchorID('IC_example')).toBe('timeline-comment-IC_example');
    expect(commentAnchorID('IC_example', 'diff')).toBe('diff-comment-IC_example');
  });

  test('preserves the pull request URL when creating a permalink', () => {
    expect(
      commentPermalink('IC_example', 'timeline', 'https://review.example/owner/repo/pull/6?view=stack'),
    ).toBe(
      'https://review.example/owner/repo/pull/6?view=stack#timeline-comment-IC_example',
    );
  });
});
