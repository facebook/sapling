/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PullRequestReviewQueryData} from './generated/graphql';
import type {GitHubReviewDeps} from './githubReview';

import {
  AddPullRequestReviewThread,
  CreatePendingPullRequestReview,
  DiffSide,
  PullRequestReviewCommentState,
  PullRequestReviewEvent,
  PullRequestReviewQuery,
  PullRequestReviewState,
  ResolvePullRequestReviewThread,
  SubmitPullRequestReview,
  UnresolvePullRequestReviewThread,
} from './generated/graphql';
import {GitHubReviewService} from './githubReview';

const repository = {
  owner: 'OpenTSLM',
  repo: 'TimeNet',
  prUrl: (diffId: string) => `https://github.com/OpenTSLM/TimeNet/pull/${diffId}`,
};

describe('GitHubReviewService', () => {
  it('maps the current viewer pending review and inline thread', async () => {
    const query = jest.fn(() => Promise.resolve(reviewResponse()));
    const service = createService(query);

    const review = await service.fetch('194');

    expect(review).toMatchObject({
      pullRequestId: 'PR_node',
      headOid: 'abc123',
      pendingReviewId: 'pending_review',
      pendingReviewCommitOid: 'abc123',
      threads: [
        {
          id: 'thread_1',
          path: 'src/model.py',
          line: 12,
          side: DiffSide.Right,
          comments: [
            {
              id: 'comment_1',
              databaseId: 42,
              author: 'reviewer',
              body: 'Please explain this branch.',
              state: PullRequestReviewCommentState.Pending,
            },
          ],
        },
      ],
    });
    expect(query).toHaveBeenCalledWith(PullRequestReviewQuery, {
      url: repository.prUrl('194'),
      numToFetch: 100,
    });
  });

  it('anchors a single comment to the selected commit', async () => {
    const query = jest.fn(() => Promise.resolve(reviewResponse()));
    const rest = jest.fn(() => Promise.resolve());
    const service = createService(query, rest);

    await service.runAction('194', {
      type: 'createComment',
      body: '  This needs a test.  ',
      path: 'src/model.py',
      line: 12,
      side: 'RIGHT',
      mode: 'single',
      commitOid: 'selected123',
      expectedHeadOid: 'abc123',
    });

    expect(rest).toHaveBeenCalledWith({
      method: 'POST',
      endpoint: 'repos/OpenTSLM/TimeNet/pulls/194/comments',
      fields: {
        body: 'This needs a test.',
        commit_id: 'selected123',
        path: 'src/model.py',
        line: 12,
        side: 'RIGHT',
      },
    });
    expect(query).toHaveBeenCalledTimes(2);
  });

  it('anchors a single comment to a selected line range', async () => {
    const query = jest.fn(() => Promise.resolve(reviewResponse()));
    const rest = jest.fn(() => Promise.resolve());
    const service = createService(query, rest);

    await service.runAction('194', {
      type: 'createComment',
      body: 'These lines belong together.',
      path: 'src/model.py',
      startLine: 10,
      startSide: 'RIGHT',
      line: 12,
      side: 'RIGHT',
      mode: 'single',
      commitOid: 'selected123',
      expectedHeadOid: 'abc123',
    });

    expect(rest).toHaveBeenCalledWith({
      method: 'POST',
      endpoint: 'repos/OpenTSLM/TimeNet/pulls/194/comments',
      fields: {
        body: 'These lines belong together.',
        commit_id: 'selected123',
        path: 'src/model.py',
        start_line: 10,
        start_side: 'RIGHT',
        line: 12,
        side: 'RIGHT',
      },
    });
  });

  it('does not comment after the pull request head changes', async () => {
    const query = jest.fn(() => Promise.resolve(reviewResponse()));
    const rest = jest.fn(() => Promise.resolve());
    const service = createService(query, rest);

    await expect(
      service.runAction('194', {
        type: 'createComment',
        body: 'This was written against older code.',
        path: 'src/model.py',
        line: 12,
        side: 'RIGHT',
        mode: 'single',
        commitOid: 'older_commit',
        expectedHeadOid: 'older_head',
      }),
    ).rejects.toThrow('changed while it was open');
    expect(rest).not.toHaveBeenCalled();
  });

  it('uses the GitHub review comment endpoints for replies, edits, and deletes', async () => {
    const query = jest.fn(() => Promise.resolve(reviewResponse()));
    const rest = jest.fn(() => Promise.resolve());
    const service = createService(query, rest);

    await service.runAction('194', {
      type: 'reply',
      body: '  Updated in the next patch.  ',
      commentDatabaseId: 42,
    });
    await service.runAction('194', {
      type: 'editComment',
      body: '  Please add a focused regression test.  ',
      commentDatabaseId: 42,
    });
    await service.runAction('194', {type: 'deleteComment', commentDatabaseId: 42});

    expect(rest.mock.calls).toEqual([
      [
        {
          method: 'POST',
          endpoint: 'repos/OpenTSLM/TimeNet/pulls/194/comments/42/replies',
          fields: {body: 'Updated in the next patch.'},
        },
      ],
      [
        {
          method: 'PATCH',
          endpoint: 'repos/OpenTSLM/TimeNet/pulls/comments/42',
          fields: {body: 'Please add a focused regression test.'},
        },
      ],
      [
        {
          method: 'DELETE',
          endpoint: 'repos/OpenTSLM/TimeNet/pulls/comments/42',
        },
      ],
    ]);
  });

  it('resolves and reopens a review thread', async () => {
    const query = jest.fn((document: string) => {
      if (document === ResolvePullRequestReviewThread) {
        return Promise.resolve({resolveReviewThread: {thread: {id: 'thread_1', isResolved: true}}});
      }
      if (document === UnresolvePullRequestReviewThread) {
        return Promise.resolve({
          unresolveReviewThread: {thread: {id: 'thread_1', isResolved: false}},
        });
      }
      return Promise.resolve(reviewResponse());
    });
    const service = createService(query);

    await service.runAction('194', {type: 'setResolved', threadId: 'thread_1', resolved: true});
    await service.runAction('194', {type: 'setResolved', threadId: 'thread_1', resolved: false});

    expect(query).toHaveBeenCalledWith(ResolvePullRequestReviewThread, {threadId: 'thread_1'});
    expect(query).toHaveBeenCalledWith(UnresolvePullRequestReviewThread, {threadId: 'thread_1'});
  });

  it('creates a pending review before its first pending thread', async () => {
    const query = jest.fn((document: string) => {
      if (document === PullRequestReviewQuery) {
        return Promise.resolve(reviewResponse(false));
      }
      if (document === CreatePendingPullRequestReview) {
        return Promise.resolve({
          addPullRequestReview: {pullRequestReview: {id: 'new_pending_review'}},
        });
      }
      if (document === AddPullRequestReviewThread) {
        return Promise.resolve({addPullRequestReviewThread: {thread: {id: 'new_thread'}}});
      }
      return Promise.reject(new Error('Unexpected GraphQL document'));
    });
    const service = createService(query);

    await service.runAction('194', {
      type: 'createComment',
      body: 'Keep this pending.',
      path: 'src/model.py',
      startLine: 10,
      line: 12,
      side: 'RIGHT',
      mode: 'pending',
      commitOid: 'selected123',
      expectedHeadOid: 'abc123',
    });

    expect(query).toHaveBeenCalledWith(CreatePendingPullRequestReview, {
      pullRequestId: 'PR_node',
      commitOid: 'selected123',
    });
    expect(query).toHaveBeenCalledWith(AddPullRequestReviewThread, {
      pullRequestReviewId: 'new_pending_review',
      body: 'Keep this pending.',
      path: 'src/model.py',
      startLine: 10,
      startSide: DiffSide.Right,
      line: 12,
      side: DiffSide.Right,
    });
  });

  it('omits range fields from a single-line pending comment', async () => {
    const query = jest.fn((document: string) => {
      if (document === PullRequestReviewQuery) {
        return Promise.resolve(reviewResponse(false));
      }
      if (document === CreatePendingPullRequestReview) {
        return Promise.resolve({
          addPullRequestReview: {pullRequestReview: {id: 'new_pending_review'}},
        });
      }
      if (document === AddPullRequestReviewThread) {
        return Promise.resolve({addPullRequestReviewThread: {thread: {id: 'new_thread'}}});
      }
      return Promise.reject(new Error('Unexpected GraphQL document'));
    });
    const service = createService(query);

    await service.runAction('194', {
      type: 'createComment',
      body: 'Keep this line pending.',
      path: 'src/model.py',
      line: 12,
      side: 'RIGHT',
      mode: 'pending',
      commitOid: 'selected123',
      expectedHeadOid: 'abc123',
    });

    expect(query).toHaveBeenCalledWith(AddPullRequestReviewThread, {
      pullRequestReviewId: 'new_pending_review',
      body: 'Keep this line pending.',
      path: 'src/model.py',
      line: 12,
      side: DiffSide.Right,
    });
  });

  it('does not mix comments from different commits in one pending review', async () => {
    const query = jest.fn(() => Promise.resolve(reviewResponse()));
    const service = createService(query);

    await expect(
      service.runAction('194', {
        type: 'createComment',
        body: 'This belongs to an earlier commit.',
        path: 'src/model.py',
        line: 12,
        side: 'LEFT',
        mode: 'pending',
        commitOid: 'selected123',
        expectedHeadOid: 'abc123',
      }),
    ).rejects.toThrow('pending GitHub review belongs to a different commit');
    expect(query).not.toHaveBeenCalledWith(AddPullRequestReviewThread, expect.anything());
  });

  it('submits the current viewer pending review', async () => {
    const query = jest.fn((document: string) => {
      if (document === PullRequestReviewQuery) {
        return Promise.resolve(reviewResponse());
      }
      if (document === SubmitPullRequestReview) {
        return Promise.resolve({
          submitPullRequestReview: {pullRequestReview: {id: 'pending_review'}},
        });
      }
      return Promise.reject(new Error('Unexpected GraphQL document'));
    });
    const service = createService(query);

    await service.runAction('194', {
      type: 'submitReview',
      event: 'APPROVE',
      body: 'The implementation looks good.',
      expectedHeadOid: 'abc123',
    });

    expect(query).toHaveBeenCalledWith(SubmitPullRequestReview, {
      pullRequestReviewId: 'pending_review',
      event: PullRequestReviewEvent.Approve,
      body: 'The implementation looks good.',
    });
  });
});

function createService(query: jest.Mock, rest = jest.fn(() => Promise.resolve())) {
  return new GitHubReviewService(repository, {
    query,
    rest,
  } as GitHubReviewDeps);
}

function reviewResponse(hasPendingReview = true): PullRequestReviewQueryData {
  return {
    viewer: {login: 'reviewer'},
    resource: {
      __typename: 'PullRequest',
      id: 'PR_node',
      headRefOid: 'abc123',
      reviews: {
        nodes: hasPendingReview
          ? [
              {
                id: 'pending_review',
                state: PullRequestReviewState.Pending,
                commit: {oid: 'abc123'},
                author: {__typename: 'User', login: 'reviewer'},
              },
            ]
          : [],
      },
      reviewThreads: {
        nodes: [
          {
            id: 'thread_1',
            path: 'src/model.py',
            line: 12,
            originalLine: 12,
            startLine: null,
            originalStartLine: null,
            diffSide: DiffSide.Right,
            startDiffSide: null,
            isOutdated: false,
            isResolved: false,
            viewerCanReply: true,
            viewerCanResolve: true,
            viewerCanUnresolve: false,
            comments: {
              nodes: [
                {
                  id: 'comment_1',
                  databaseId: 42,
                  body: 'Please explain this branch.',
                  bodyHTML: '<p>Please explain this branch.</p>',
                  createdAt: '2026-09-01T10:00:00Z',
                  publishedAt: null,
                  state: PullRequestReviewCommentState.Pending,
                  url: 'https://github.com/OpenTSLM/TimeNet/pull/194#discussion_r42',
                  viewerCanDelete: true,
                  viewerCanUpdate: true,
                  author: {
                    __typename: 'User',
                    login: 'reviewer',
                    avatarUrl: 'https://avatars.example/reviewer',
                  },
                  reactions: {nodes: []},
                },
              ],
            },
          },
        ],
      },
    },
  };
}
