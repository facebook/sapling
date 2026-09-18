/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {
  ReviewCommentFetchCache,
  reviewCommentBody,
  reviewStackPullRequestUrl,
  suggestionBody,
} from '../GitHubReviewCommentsProvider';

describe('GitHub review comment fetching', () => {
  it('shares concurrent and recent requests for the same pull request', async () => {
    let now = 1_000;
    const cache = new ReviewCommentFetchCache(60_000, () => now);
    const fetchComments = jest.fn(() => Promise.resolve([]));

    const first = cache.get('repo\0pr', fetchComments);
    const second = cache.get('repo\0pr', fetchComments);
    await expect(Promise.all([first, second])).resolves.toEqual([[], []]);
    expect(fetchComments).toHaveBeenCalledTimes(1);

    now += 30_000;
    await cache.get('repo\0pr', fetchComments);
    expect(fetchComments).toHaveBeenCalledTimes(1);

    now += 60_000;
    await cache.get('repo\0pr', fetchComments);
    expect(fetchComments).toHaveBeenCalledTimes(2);
  });

  it('does not cache failed requests', async () => {
    const cache = new ReviewCommentFetchCache();
    const fetchComments = jest
      .fn<Promise<never>, []>()
      .mockRejectedValueOnce(new Error('rate limited'))
      .mockRejectedValueOnce(new Error('still rate limited'));

    await expect(cache.get('repo\0pr', fetchComments)).rejects.toThrow('rate limited');
    await expect(cache.get('repo\0pr', fetchComments)).rejects.toThrow('still rate limited');
    expect(fetchComments).toHaveBeenCalledTimes(2);
  });
});

describe('GitHub review suggestions', () => {
  it('copies selected lines into a GitHub suggestion block', () => {
    expect(suggestionBody('const one = 1;\nconst two = 2;')).toBe(
      '```suggestion\nconst one = 1;\nconst two = 2;\n```',
    );
  });

  it('preserves an existing comment before the suggestion', () => {
    expect(suggestionBody('return result;', 'Please simplify this.')).toBe(
      'Please simplify this.\n\n```suggestion\nreturn result;\n```',
    );
  });
});

describe('GitHub review comment display', () => {
  it('puts GitHub and ReviewStack links above the body', () => {
    expect(
      reviewCommentBody(
        'Looks good.',
        'https://github.com/o/r/pull/1#discussion_r2',
        'https://reviewstack.dev/o/r/pull/1',
      ),
    ).toBe(
      `[View on GitHub](https://github.com/o/r/pull/1#discussion_r2) · [View in ReviewStack](https://reviewstack.dev/o/r/pull/1)\n\nLooks good.`,
    );
  });

  it('creates a ReviewStack URL using a configured server', () => {
    expect(reviewStackPullRequestUrl('o', 'r', '42', 'reviews.example.com/')).toBe(
      'https://reviews.example.com/o/r/pull/42',
    );
  });
});
