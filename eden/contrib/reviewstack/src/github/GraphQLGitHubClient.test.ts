/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import GraphQLGitHubClient from './GraphQLGitHubClient';

describe('GraphQLGitHubClient comment mutations', () => {
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
});
