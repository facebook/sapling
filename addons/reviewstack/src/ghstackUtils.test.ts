/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {pullRequestNumbersFromBody, stripStackInfoFromBodyHTML} from './ghstackUtils';

describe('pullRequestNumbersFromBody', () => {
  test('returns array of numbers for pull requests listed in the body', () => {
    const body = `\
Stack from [ghstack](https://github.com/ezyang/ghstack):
* __->__ #80477
* #80418
* #80416

This is the commit message.
`;
    expect(pullRequestNumbersFromBody(body)).toEqual([80477, 80418, 80416]);
  });

  test('does not parse PRs if ghstack header is not present', () => {
    const body = `\
Review stack:
* __->__ #80477
* #80418
* #80416

This is the commit message.
`;
    expect(pullRequestNumbersFromBody(body)).toBe(null);
  });

  test('ignores bullet points that are part of the commit message or not inserted by ghstack', () => {
    const body = `\
Stack from [ghstack](https://github.com/ezyang/ghstack):
* __->__ #80477
* #80418
* #80416

This is the commit message with some bullets.
* Item #1
* Item #2
`;
    expect(pullRequestNumbersFromBody(body)).toEqual([80477, 80418, 80416]);
  });

  test('handles body that does not contain a commit message', () => {
    const body = `\
Stack from [ghstack](https://github.com/ezyang/ghstack):
* __->__ #80477
* #80418
* #80416
`;
    expect(pullRequestNumbersFromBody(body)).toEqual([80477, 80418, 80416]);
  });

  test('handles body that includes carriage return characters', () => {
    const body = `\
Stack from [ghstack](https://github.com/ezyang/ghstack):\r
* __->__ #80477\r
* #80418\r
* #80416\r
\r
This is the commit message with some bullets.\r
* Item #1\r
* Item #2\r
`;
    expect(pullRequestNumbersFromBody(body)).toEqual([80477, 80418, 80416]);
  });
});

describe('stripStackInfoFromBodyHTML', () => {
  test('returns body HTML without the stack info inserted by ghstack', () => {
    const bodyHTML = `\
<p dir="auto">Stack from <a href="https://github.com/ezyang/ghstack">ghstack</a>:</p>
<ul>
<li><strong>-&gt;</strong> <a href="https://github.com/pytorch/pytorch/pull/80477">#80477</a></li>
<li><a href="https://github.com/pytorch/pytorch/pull/80418">#80418</a></li>
<li><a href="https://github.com/pytorch/pytorch/pull/80416">#80416</a></li>
</ul>
<p dir="auto">Adding a test for open device registration using cpp extensions.</p>
`;
    const expected = `\
<p dir="auto">Adding a test for open device registration using cpp extensions.</p>
`;

    expect(stripStackInfoFromBodyHTML(bodyHTML)).toEqual(expected);
  });

  test('handles body with bullet points', () => {
    const bodyHTML = `\
<p dir="auto">Stack from <a href="https://github.com/ezyang/ghstack">ghstack</a>:</p>
<ul>
<li><strong>-&gt;</strong> <a href="https://github.com/pytorch/pytorch/pull/80477">#80477</a></li>
<li><a href="https://github.com/pytorch/pytorch/pull/80418">#80418</a></li>
<li><a href="https://github.com/pytorch/pytorch/pull/80416">#80416</a></li>
</ul>
<ul dir="auto">
<li>Item #1</li>
<li>Item #2</li>
</ul>
`;
    const expected = `\
<ul dir="auto">
<li>Item #1</li>
<li>Item #2</li>
</ul>
`;

    expect(stripStackInfoFromBodyHTML(bodyHTML)).toEqual(expected);
  });
});
