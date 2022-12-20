/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {parseSaplingStackBody} from './saplingStack';

describe('parseSaplingStackBody', () => {
  test('extract all sections', () => {
    const parsedBody = parseSaplingStackBody(`\
Stack created with [Sapling](https://sapling-scm.com/github).

Stacks created by Sapling are best viewed using ReviewStack
so that each commit in the stack can be reviewed individually.

* #1
* __->__ #123
* #456
* #789 (2 commits)

This would be the original commit message of this fictitious commit.
`);
    expect(parsedBody).toEqual({
      firstLine: 'Stack created with [Sapling](https://sapling-scm.com/github).',
      introduction: `\

Stacks created by Sapling are best viewed using ReviewStack
so that each commit in the stack can be reviewed individually.
`,
      stack: [
        {number: 1, numCommits: 1},
        {number: 123, numCommits: 1},
        {number: 456, numCommits: 1},
        {number: 789, numCommits: 2},
      ],
      currentStackEntry: 1,
      commitMessage: 'This would be the original commit message of this fictitious commit.\n',
    });
  });

  test('extract all sections when horizontal rule is used', () => {
    const parsedBody = parseSaplingStackBody(`\
This would be the original commit message of this fictitious commit.
---
Stack created with [Sapling](https://sapling-scm.com/github).

Stacks created by Sapling are best viewed using ReviewStack
so that each commit in the stack can be reviewed individually.

* #1
* __->__ #123
* #456
* #789 (2 commits)
`);
    expect(parsedBody).toEqual({
      firstLine: 'Stack created with [Sapling](https://sapling-scm.com/github).',
      introduction: `\

Stacks created by Sapling are best viewed using ReviewStack
so that each commit in the stack can be reviewed individually.
`,
      stack: [
        {number: 1, numCommits: 1},
        {number: 123, numCommits: 1},
        {number: 456, numCommits: 1},
        {number: 789, numCommits: 2},
      ],
      currentStackEntry: 1,
      commitMessage: 'This would be the original commit message of this fictitious commit.\n',
    });
  });

  test('horizontal rule with empty commit message', () => {
    const parsedBody = parseSaplingStackBody(`\
---
Stack created with [Sapling](https://sapling-scm.com/github).

* #1
* __->__ #123
`);
    expect(parsedBody).toEqual({
      firstLine: 'Stack created with [Sapling](https://sapling-scm.com/github).',
      introduction: '',
      stack: [
        {number: 1, numCommits: 1},
        {number: 123, numCommits: 1},
      ],
      currentStackEntry: 1,
      commitMessage: '',
    });
  });

  test('pull request body with no bullet points does not parse', () => {
    const parsedBody = parseSaplingStackBody(`\
Stack created with [Sapling](https://sapling-scm.com/github).
`);
    expect(parsedBody).toBe(null);
  });

  test('pull request body with no selected PR does not parse', () => {
    const parsedBody = parseSaplingStackBody(`\
Stack created with [Sapling](https://sapling-scm.com/github).

* #1
* #123
* #456
* #789 (2 commits)
`);
    expect(parsedBody).toBe(null);
  });

  test('pull request body with multiple PRs selected does not parse', () => {
    const parsedBody = parseSaplingStackBody(`\
Stack created with [Sapling](https://sapling-scm.com/github).

* __->__ #1
* #123
* __->__ #456
* #789 (2 commits)
`);
    expect(parsedBody).toBe(null);
  });

  test('not a Sapling stack pull request body', () => {
    const parsedBody = parseSaplingStackBody('hello world');
    expect(parsedBody).toBe(null);
  });
});
