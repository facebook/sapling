/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {GitHubPullRequestReviewThread} from './github/pullRequestTimelineTypes';
import type {Version} from './github/types';

import reviewThreadsForVersion from './reviewThreadsForVersion';

function thread(id: string, commit: string, path = 'src/example.ts') {
  return {
    id,
    diffSide: 'RIGHT',
    comments: [{id: `${id}-comment`, originalCommit: {oid: commit}, path}],
  } as GitHubPullRequestReviewThread;
}

test('shows current and earlier comments while excluding later versions and other files', () => {
  const versions = [
    {commits: [{commit: 'v1'}]},
    {commits: [{commit: 'v2'}]},
    {commits: [{commit: 'v3'}]},
  ] as Version[];
  const v1 = thread('v1-thread', 'v1');
  const v2 = thread('v2-thread', 'v2');
  const v3 = thread('v3-thread', 'v3');
  const otherFile = thread('other-file', 'v1', 'src/other.ts');

  const result = reviewThreadsForVersion([v1, v2, v3, otherFile], versions, 'v2', 'src/example.ts');

  expect(result?.LEFT).toEqual([]);
  expect(result?.RIGHT).toEqual([
    expect.objectContaining({id: 'v1-thread', sourceVersionIndex: 0, isHistorical: true}),
    expect.objectContaining({id: 'v2-thread', sourceVersionIndex: 1, isHistorical: false}),
  ]);
});

test('maps a comment through the version head when it is absent from the commit list', () => {
  const versions = [
    {headCommit: 'v1-head', commits: [{commit: 'v1-rewritten'}]},
    {headCommit: 'v2-head', commits: [{commit: 'v2-rewritten'}]},
  ] as Version[];
  const historical = thread('historical', 'v1-head');

  const result = reviewThreadsForVersion(
    [historical],
    versions,
    'v2-head',
    'src/example.ts',
  );

  expect(result?.RIGHT).toEqual([
    expect.objectContaining({id: 'historical', sourceVersionIndex: 0, isHistorical: true}),
  ]);
});
