/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Logger} from '../../logger';

import {GitHubCodeReviewProvider} from '../githubCodeReviewProvider';
import queryGraphQL from '../queryGraphQL';
import queryREST from '../queryREST';

jest.mock('../queryGraphQL');
jest.mock('../queryREST');

const mockQueryGraphQL = queryGraphQL as jest.MockedFunction<typeof queryGraphQL>;
const mockQueryREST = queryREST as jest.MockedFunction<typeof queryREST>;

const provider = new GitHubCodeReviewProvider(
  {type: 'github', hostname: 'github.com', owner: 'owner', repo: 'repo'},
  {info: jest.fn(), error: jest.fn()} as unknown as Logger,
);

describe('GitHubCodeReviewProvider comments', () => {
  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('maps multiline threads and replies from GitHub', async () => {
    mockQueryGraphQL.mockResolvedValue({
      resource: {
        __typename: 'PullRequest',
        comments: {nodes: []},
        reviewThreads: {
          nodes: [
            {
              id: 'thread-id',
              isResolved: false,
              path: 'src/example.ts',
              line: 12,
              startLine: 10,
              originalLine: 12,
              originalStartLine: 10,
              diffSide: 'RIGHT',
              comments: {
                nodes: [
                  {
                    id: 'node-1',
                    databaseId: 101,
                    body: 'Please change this.',
                    bodyHTML: '<p>Please change this.</p>',
                    createdAt: '2026-09-16T10:00:00Z',
                    author: {login: 'reviewer', avatarUrl: 'https://example.com/avatar'},
                    reactions: {nodes: []},
                    line: 12,
                    startLine: 10,
                    path: 'src/example.ts',
                  },
                  {
                    id: 'node-2',
                    databaseId: 102,
                    body: 'Done.',
                    bodyHTML: '<p>Done.</p>',
                    createdAt: '2026-09-16T10:01:00Z',
                    author: {login: 'author', avatarUrl: 'https://example.com/author'},
                    reactions: {nodes: []},
                    line: 12,
                    startLine: 10,
                    path: 'src/example.ts',
                  },
                ],
              },
            },
          ],
        },
      },
    } as never);

    await expect(provider.fetchComments('42')).resolves.toMatchObject([
      {
        id: '101',
        url: 'https://github.com/owner/repo/pull/42#discussion_r101',
        content: 'Please change this.',
        filename: 'src/example.ts',
        startLine: 10,
        line: 12,
        side: 'RIGHT',
        isResolved: false,
        replies: [
          {
            id: '102',
            url: 'https://github.com/owner/repo/pull/42#discussion_r102',
            content: 'Done.',
          },
        ],
      },
    ]);
    expect(mockQueryGraphQL.mock.calls[0]?.[1]).toMatchObject({
      includeReactions: true,
      numToFetch: 50,
    });
  });

  it('posts a multiline comment against the latest pull request head', async () => {
    mockQueryREST.mockResolvedValueOnce({head: {sha: 'head-sha'}} as never);
    mockQueryREST.mockResolvedValueOnce({
      id: 123,
      html_url: 'https://github.com/owner/repo/pull/42#discussion_r123',
      body: '```suggestion\nreplacement\n```',
      created_at: '2026-09-16T10:00:00Z',
      user: {login: 'reviewer', avatar_url: 'https://example.com/avatar'},
    } as never);

    const created = await provider.createInlineComment('42', {
      body: '```suggestion\nreplacement\n```',
      path: 'src/example.ts',
      startLine: 10,
      line: 12,
      side: 'RIGHT',
    });

    expect(mockQueryREST).toHaveBeenNthCalledWith(
      2,
      'repos/owner/repo/pulls/42/comments',
      'github.com',
      'POST',
      {
        body: '```suggestion\nreplacement\n```',
        commit_id: 'head-sha',
        path: 'src/example.ts',
        start_line: 10,
        start_side: 'RIGHT',
        line: 12,
        side: 'RIGHT',
      },
    );
    expect(created).toEqual({
      id: '123',
      url: 'https://github.com/owner/repo/pull/42#discussion_r123',
      body: '```suggestion\nreplacement\n```',
      author: 'reviewer',
      authorAvatarUri: 'https://example.com/avatar',
      created: new Date('2026-09-16T10:00:00Z'),
    });
  });

  it('posts replies without looking up the pull request head', async () => {
    mockQueryREST.mockResolvedValue({} as never);

    await provider.createInlineComment('42', {
      body: 'Reply',
      path: 'src/example.ts',
      line: 12,
      side: 'RIGHT',
      replyTo: '101',
    });

    expect(mockQueryREST).toHaveBeenCalledTimes(1);
    expect(mockQueryREST).toHaveBeenCalledWith(
      'repos/owner/repo/pulls/42/comments',
      'github.com',
      'POST',
      {body: 'Reply', in_reply_to: 101},
    );
  });
});

describe('GitHubCodeReviewProvider summaries', () => {
  beforeEach(() => {
    jest.useFakeTimers();
    jest.setSystemTime(new Date('2026-09-17T06:00:00Z'));
    jest.clearAllMocks();
  });

  afterEach(() => {
    jest.useRealTimers();
  });

  it('throttles automatic refreshes while allowing a forced refresh', async () => {
    mockQueryGraphQL.mockResolvedValueOnce({__type: null} as never).mockResolvedValue({
      search: {nodes: []},
    } as never);
    const summariesProvider = new GitHubCodeReviewProvider(
      {type: 'github', hostname: 'github.com', owner: 'owner', repo: 'repo'},
      {info: jest.fn(), error: jest.fn()} as unknown as Logger,
    );

    summariesProvider.triggerDiffSummariesFetch([]);
    await jest.runAllTimersAsync();
    expect(mockQueryGraphQL).toHaveBeenCalledTimes(2);
    expect(mockQueryGraphQL.mock.calls[1][0]).toContain('commits(last: 1)');

    summariesProvider.triggerDiffSummariesFetch([]);
    await jest.runAllTimersAsync();
    expect(mockQueryGraphQL).toHaveBeenCalledTimes(2);

    summariesProvider.triggerDiffSummariesFetch([], true);
    await jest.runAllTimersAsync();
    expect(mockQueryGraphQL).toHaveBeenCalledTimes(3);

    jest.advanceTimersByTime(5 * 60_000);
    summariesProvider.triggerDiffSummariesFetch([]);
    await jest.runAllTimersAsync();
    expect(mockQueryGraphQL).toHaveBeenCalledTimes(4);

    summariesProvider.dispose();
  });
});
