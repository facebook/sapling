/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {isGraphiteStackComment, parseGraphiteStackComment} from './graphiteStack';

const GRAPHITE_COMMENT = `\
> [!WARNING]
> <b>This pull request is not mergeable via GitHub because a downstack PR is open.</b>

* **#3818** <a href="https://app.graphite.com/github/pr/org/repo/3818">👈</a>
* **#3815** <a href="https://app.graphite.com/github/pr/org/repo/3815"></a>
* **#3814** <a href="https://app.graphite.com/github/pr/org/repo/3814"></a>
* \`main\`

---

This stack of pull requests is managed by **Graphite**.
`;

const GRAPHITE_COMMENT_MIDDLE_OF_STACK = `\
* **#3818** <a href="https://app.graphite.com/github/pr/org/repo/3818"></a>
* **#3815** <a href="https://app.graphite.com/github/pr/org/repo/3815">👈</a>
* **#3814** <a href="https://app.graphite.com/github/pr/org/repo/3814"></a>
* \`main\`

This stack of pull requests is managed by **Graphite**.
`;

const GRAPHITE_COMMENT_BOTTOM_OF_STACK = `\
* **#3818** <a href="https://app.graphite.com/github/pr/org/repo/3818"></a>
* **#3815** <a href="https://app.graphite.com/github/pr/org/repo/3815"></a>
* **#3814** <a href="https://app.graphite.com/github/pr/org/repo/3814">👈</a>
* \`main\`

This stack of pull requests is managed by **Graphite**.
`;

const GRAPHITE_COMMENT_SINGLE_PR = `\
* **#100** <a href="https://app.graphite.com/github/pr/org/repo/100">👈</a>
* \`develop\`

This stack of pull requests is managed by **Graphite**.
`;

const NON_GRAPHITE_COMMENT = `\
This is a regular comment that doesn't contain any Graphite stack info.
Here are some PR references: #123 #456
`;

describe('isGraphiteStackComment', () => {
  test('returns true for Graphite stack comment', () => {
    expect(isGraphiteStackComment(GRAPHITE_COMMENT)).toBe(true);
  });

  test('returns false for non-Graphite comment', () => {
    expect(isGraphiteStackComment(NON_GRAPHITE_COMMENT)).toBe(false);
  });

  test('returns false for empty string', () => {
    expect(isGraphiteStackComment('')).toBe(false);
  });
});

describe('parseGraphiteStackComment', () => {
  test('parses stack with current PR at top', () => {
    const result = parseGraphiteStackComment(GRAPHITE_COMMENT);
    expect(result).toEqual({
      stack: [3818, 3815, 3814],
      currentStackEntry: 0,
      baseBranch: 'main',
    });
  });

  test('parses stack with current PR in middle', () => {
    const result = parseGraphiteStackComment(GRAPHITE_COMMENT_MIDDLE_OF_STACK);
    expect(result).toEqual({
      stack: [3818, 3815, 3814],
      currentStackEntry: 1,
      baseBranch: 'main',
    });
  });

  test('parses stack with current PR at bottom', () => {
    const result = parseGraphiteStackComment(GRAPHITE_COMMENT_BOTTOM_OF_STACK);
    expect(result).toEqual({
      stack: [3818, 3815, 3814],
      currentStackEntry: 2,
      baseBranch: 'main',
    });
  });

  test('parses single PR stack', () => {
    const result = parseGraphiteStackComment(GRAPHITE_COMMENT_SINGLE_PR);
    expect(result).toEqual({
      stack: [100],
      currentStackEntry: 0,
      baseBranch: 'develop',
    });
  });

  test('returns null for non-Graphite comment', () => {
    const result = parseGraphiteStackComment(NON_GRAPHITE_COMMENT);
    expect(result).toBe(null);
  });

  test('returns null for empty string', () => {
    const result = parseGraphiteStackComment('');
    expect(result).toBe(null);
  });

  test('returns null for Graphite comment without current PR marker', () => {
    const commentWithoutMarker = `\
* **#3818** <a href="..."></a>
* **#3815** <a href="..."></a>
* \`main\`

This stack of pull requests is managed by **Graphite**.
`;
    const result = parseGraphiteStackComment(commentWithoutMarker);
    expect(result).toBe(null);
  });

  test('handles comment without base branch', () => {
    const commentWithoutBase = `\
* **#3818** <a href="...">👈</a>
* **#3815** <a href="..."></a>

This stack of pull requests is managed by **Graphite**.
`;
    const result = parseGraphiteStackComment(commentWithoutBase);
    expect(result).toEqual({
      stack: [3818, 3815],
      currentStackEntry: 0,
      baseBranch: null,
    });
  });

  test('handles carriage return characters', () => {
    const commentWithCR = `\
* **#3818** <a href="...">👈</a>\r
* **#3815** <a href="..."></a>\r
* \`main\`\r
\r
This stack of pull requests is managed by **Graphite**.\r
`;
    const result = parseGraphiteStackComment(commentWithCR);
    expect(result).toEqual({
      stack: [3818, 3815],
      currentStackEntry: 0,
      baseBranch: 'main',
    });
  });
});
