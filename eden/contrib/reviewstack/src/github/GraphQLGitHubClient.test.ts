/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import GraphQLGitHubClient, {treesFromRecursiveResponse} from './GraphQLGitHubClient';
import {DiffSide, ReactionContent} from '../generated/graphql';

describe('recursive Git tree prefetch', () => {
  test('builds directly addressable trees while preserving sorted entries', () => {
    const trees = treesFromRecursiveResponse('root', [
      {mode: '100644', path: 'z.txt', sha: 'z', type: 'blob'},
      {mode: '040000', path: 'src', sha: 'src-tree', type: 'tree'},
      {mode: '100644', path: 'src/a.ts', sha: 'a', type: 'blob'},
    ]);

    expect(trees).toEqual([
      {
        id: 'root',
        oid: 'root',
        entries: [
          {mode: 0o40000, name: 'src', oid: 'src-tree', path: 'src', type: 'tree'},
          {mode: 0o100644, name: 'z.txt', oid: 'z', path: 'z.txt', type: 'blob'},
        ],
      },
      {
        id: 'src-tree',
        oid: 'src-tree',
        entries: [
          {mode: 0o100644, name: 'a.ts', oid: 'a', path: 'src/a.ts', type: 'blob'},
        ],
      },
    ]);
  });

  test('serves subtrees without another GitHub request after prefetch', async () => {
    const fetchMock = jest.spyOn(globalThis, 'fetch').mockResolvedValue({
      ok: true,
      json: () =>
        Promise.resolve({
          sha: 'root',
          tree: [
            {mode: '040000', path: 'src', sha: 'src-tree', type: 'tree'},
            {mode: '100644', path: 'src/a.ts', sha: 'a', type: 'blob'},
          ],
          truncated: false,
        }),
    } as Response);
    const client = new GraphQLGitHubClient('github.com', 'owner', 'repo', 'token');

    await client.prefetchTree('root');
    await expect(client.getTree('src-tree')).resolves.toEqual({
      id: 'src-tree',
      oid: 'src-tree',
      entries: [{mode: 0o100644, name: 'a.ts', oid: 'a', path: 'src/a.ts', type: 'blob'}],
    });
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(fetchMock.mock.calls[0][0]).toBe(
      'https://api.github.com/repos/owner/repo/git/trees/root?recursive=1',
    );

    fetchMock.mockRestore();
  });
});

describe('commit comparisons', () => {
  test('keeps per-file line totals from the REST response', async () => {
    const fetchMock = jest.spyOn(globalThis, 'fetch').mockResolvedValue({
      ok: true,
      status: 200,
      json: () =>
        Promise.resolve({
          commits: [],
          files: [
            {
              additions: 12,
              deletions: 4,
              filename: 'src/new.ts',
              previous_filename: 'src/old.ts',
              status: 'renamed',
            },
          ],
          merge_base_commit: {sha: 'base', commit: {committer: {date: '2026-09-17'}}},
        }),
    } as Response);
    const client = new GraphQLGitHubClient('github.com', 'owner', 'repo', 'token');

    await expect(client.getCommitComparison('base', 'head')).resolves.toEqual({
      commits: [],
      files: [
        {
          additions: 12,
          deletions: 4,
          filename: 'src/new.ts',
          previousFilename: 'src/old.ts',
          status: 'renamed',
        },
      ],
      mergeBaseCommit: {sha: 'base', commit: {committer: {date: '2026-09-17'}}},
    });

    fetchMock.mockRestore();
  });
});

describe('GraphQLGitHubClient comment mutations', () => {
  test('updates reactions and review thread resolution', async () => {
    const fetchMock = jest.spyOn(globalThis, 'fetch').mockResolvedValue({
      ok: true,
      json: () => Promise.resolve({data: {}}),
    } as Response);
    const client = new GraphQLGitHubClient('github.com', 'owner', 'repo', 'token');

    await client.addReaction({subjectId: 'comment-id', content: ReactionContent.ThumbsUp});
    await client.removeReaction({subjectId: 'comment-id', content: ReactionContent.ThumbsUp});
    await client.resolveReviewThread({threadId: 'thread-id'});
    await client.unresolveReviewThread({threadId: 'thread-id'});

    const requests = fetchMock.mock.calls.map(([, init]) => JSON.parse(String(init?.body)));
    expect(requests).toEqual([
      expect.objectContaining({
        query: expect.stringContaining('mutation AddReactionMutation'),
        variables: {input: {subjectId: 'comment-id', content: 'THUMBS_UP'}},
      }),
      expect.objectContaining({
        query: expect.stringContaining('mutation RemoveReactionMutation'),
        variables: {input: {subjectId: 'comment-id', content: 'THUMBS_UP'}},
      }),
      expect.objectContaining({
        query: expect.stringContaining('mutation ResolveReviewThreadMutation'),
        variables: {input: {threadId: 'thread-id'}},
      }),
      expect.objectContaining({
        query: expect.stringContaining('mutation UnresolveReviewThreadMutation'),
        variables: {input: {threadId: 'thread-id'}},
      }),
    ]);

    fetchMock.mockRestore();
  });

  test('updates and deletes issue and review comments', async () => {
    const fetchMock = jest.spyOn(globalThis, 'fetch').mockResolvedValue({
      ok: true,
      json: () => Promise.resolve({data: {}}),
    } as Response);
    const client = new GraphQLGitHubClient('github.com', 'owner', 'repo', 'token');

    await client.updateIssueComment({id: 'issue-id', body: 'updated issue comment'});
    await client.deleteIssueComment({id: 'issue-id'});
    await client.updatePullRequestReviewComment({
      pullRequestReviewCommentId: 'review-id',
      body: 'updated review comment',
    });
    await client.deletePullRequestReviewComment({id: 'review-id'});

    const requests = fetchMock.mock.calls.map(([, init]) => JSON.parse(String(init?.body)));
    expect(requests).toEqual([
      expect.objectContaining({
        query: expect.stringContaining('mutation UpdateIssueCommentMutation'),
        variables: {input: {id: 'issue-id', body: 'updated issue comment'}},
      }),
      expect.objectContaining({
        query: expect.stringContaining('mutation DeleteIssueCommentMutation'),
        variables: {input: {id: 'issue-id'}},
      }),
      expect.objectContaining({
        query: expect.stringContaining('mutation UpdatePullRequestReviewCommentMutation'),
        variables: {
          input: {
            pullRequestReviewCommentId: 'review-id',
            body: 'updated review comment',
          },
        },
      }),
      expect.objectContaining({
        query: expect.stringContaining('mutation DeletePullRequestReviewCommentMutation'),
        variables: {input: {id: 'review-id'}},
      }),
    ]);

    fetchMock.mockRestore();
  });

  test('adds another thread to an existing pending review', async () => {
    const fetchMock = jest.spyOn(globalThis, 'fetch').mockResolvedValue({
      ok: true,
      json: () => Promise.resolve({data: {}}),
    } as Response);
    const client = new GraphQLGitHubClient('github.com', 'owner', 'repo', 'token');

    await client.addPullRequestReviewThread({
      body: 'second comment',
      line: 12,
      path: 'src/example.ts',
      pullRequestReviewId: 'pending-review-id',
      side: DiffSide.Right,
      startLine: 10,
      startSide: DiffSide.Right,
    });

    const request = JSON.parse(String(fetchMock.mock.calls[0][1]?.body));
    expect(request).toEqual(
      expect.objectContaining({
        query: expect.stringContaining('mutation AddPullRequestReviewThreadMutation'),
        variables: {
          input: {
            body: 'second comment',
            line: 12,
            path: 'src/example.ts',
            pullRequestReviewId: 'pending-review-id',
            side: 'RIGHT',
            startLine: 10,
            startSide: 'RIGHT',
          },
        },
      }),
    );

    fetchMock.mockRestore();
  });
});

describe('GraphQLGitHubClient pull request state mutations', () => {
  test('converts a pull request between draft and ready states', async () => {
    const fetchMock = jest.spyOn(globalThis, 'fetch').mockResolvedValue({
      ok: true,
      json: () => Promise.resolve({data: {}}),
    } as Response);
    const client = new GraphQLGitHubClient('github.com', 'owner', 'repo', 'token');

    await client.convertPullRequestToDraft({pullRequestId: 'pull-request-id'});
    await client.markPullRequestReadyForReview({pullRequestId: 'pull-request-id'});

    const requests = fetchMock.mock.calls.map(([, init]) => JSON.parse(String(init?.body)));
    expect(requests).toEqual([
      expect.objectContaining({
        query: expect.stringContaining('mutation ConvertPullRequestToDraftMutation'),
        variables: {input: {pullRequestId: 'pull-request-id'}},
      }),
      expect.objectContaining({
        query: expect.stringContaining('mutation MarkPullRequestReadyForReviewMutation'),
        variables: {input: {pullRequestId: 'pull-request-id'}},
      }),
    ]);

    fetchMock.mockRestore();
  });
});

describe('GraphQLGitHubClient stack fragments', () => {
  test('skips pull requests that no longer exist', async () => {
    const pullRequest = {
      __typename: 'PullRequest' as const,
      comments: {totalCount: 0},
      headRefOid: 'head',
      isDraft: false,
      number: 6,
      reviewDecision: null,
      state: 'OPEN',
      title: 'Existing pull request',
      updatedAt: '2026-09-17T00:00:00Z',
    };
    const fetchMock = jest.spyOn(globalThis, 'fetch').mockImplementation((_url, init) => {
      const {variables} = JSON.parse(String(init?.body));
      const json =
        variables.pr === 61
          ? {
              data: {repository: {pullRequest: null}},
              errors: [
                {
                  message: 'Could not resolve to a PullRequest with the number of 61.',
                  path: ['repository', 'pullRequest'],
                  type: 'NOT_FOUND',
                },
              ],
            }
          : {data: {repository: {pullRequest}}};
      return Promise.resolve({
        headers: new Headers(),
        json: () => Promise.resolve(json),
        ok: true,
      } as Response);
    });
    const client = new GraphQLGitHubClient('github.com', 'owner', 'repo', 'token');

    await expect(client.getStackPullRequests([6, 61])).resolves.toEqual([pullRequest]);
    expect(fetchMock).toHaveBeenCalledTimes(2);

    fetchMock.mockRestore();
  });
});
