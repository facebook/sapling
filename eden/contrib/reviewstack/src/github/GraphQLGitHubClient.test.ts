/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import GraphQLGitHubClient, {treesFromRecursiveResponse} from './GraphQLGitHubClient';

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
