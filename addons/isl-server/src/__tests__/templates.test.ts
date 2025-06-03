/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CodeReviewSystem} from 'isl/src/types';
import path from 'path';
import {mockLogger} from 'shared/testUtils';
import {COMMIT_END_MARK, findMaxCommonPathPrefix, parseCommitInfoOutput} from '../templates';

const MOCK_GITHUB_REVIEW_SYSTEM: CodeReviewSystem = {
  type: 'github',
  repo: 'eden',
  hostname: 'github.com',
  owner: 'facebook',
};

describe('template parsing', () => {
  it('parses normal commits', () => {
    expect(
      parseCommitInfoOutput(
        mockLogger,
        `\
77fdcef8759fb65da46a7a6431310829f12cef5b
Commit A
Author <author@example.com>
2024-04-24 14:16:24 -0700
draft


3f41d88ab69446763404eccd0f3e579352ba2753\x00


sapling/addons/isl/README.md
1


1
false
|
Commit A

Summary:
this is my summary
${COMMIT_END_MARK}
e4594714fb9b3410a0ef4affc955f9d76d61c8a7
Commit B
Author <author@example.com>
2024-04-24 12:19:08 -0700
draft


2934650733c9181bdf64b7d00f5e0c7ca93d7ed7\x00


sapling/addons/isl/README.md\x00sapling/addons/isl/package.json
1

9637166dabea9ac50ccb93902f3f41df4d8a15c4,

false
|
Commit B
`,
        MOCK_GITHUB_REVIEW_SYSTEM,
        null,
      ),
    ).toEqual([
      {
        author: 'Author <author@example.com>',
        bookmarks: [],
        closestPredecessors: [],
        date: new Date('2024-04-24T21:16:24.000Z'),
        description: 'Summary:\nthis is my summary',
        diffId: '1',
        filePathsSample: ['sapling/addons/isl/README.md'],
        hash: '77fdcef8759fb65da46a7a6431310829f12cef5b',
        isDot: false,
        isFollower: false,
        parents: ['3f41d88ab69446763404eccd0f3e579352ba2753'],
        grandparents: [],
        phase: 'draft',
        remoteBookmarks: [],
        stableCommitMetadata: undefined,
        successorInfo: undefined,
        title: 'Commit A',
        totalFileCount: 1,
        maxCommonPathPrefix: 'sapling/addons/isl/',
      },
      {
        author: 'Author <author@example.com>',
        bookmarks: [],
        closestPredecessors: ['9637166dabea9ac50ccb93902f3f41df4d8a15c4'],
        date: new Date('2024-04-24T19:19:08.000Z'),
        description: '',
        diffId: undefined,
        filePathsSample: ['sapling/addons/isl/README.md', 'sapling/addons/isl/package.json'],
        hash: 'e4594714fb9b3410a0ef4affc955f9d76d61c8a7',
        isDot: false,
        isFollower: false,
        parents: ['2934650733c9181bdf64b7d00f5e0c7ca93d7ed7'],
        grandparents: [],
        phase: 'draft',
        remoteBookmarks: [],
        stableCommitMetadata: undefined,
        successorInfo: undefined,
        title: 'Commit B',
        totalFileCount: 1,
        maxCommonPathPrefix: 'sapling/addons/isl/',
      },
    ]);
  });

  it('handles commits with no title+description ', () => {
    expect(
      parseCommitInfoOutput(
        mockLogger,
        `\
77fdcef8759fb65da46a7a6431310829f12cef5b

Author <author@example.com>
2024-04-24 14:16:24 -0700
draft


3f41d88ab69446763404eccd0f3e579352ba2753\x00


sapling/addons/isl/README.md
1


1
false
|
${COMMIT_END_MARK}
`,
        MOCK_GITHUB_REVIEW_SYSTEM,
        null,
      ),
    ).toEqual([
      {
        author: 'Author <author@example.com>',
        bookmarks: [],
        closestPredecessors: [],
        date: new Date('2024-04-24T21:16:24.000Z'),
        description: '',
        diffId: '1',
        filePathsSample: ['sapling/addons/isl/README.md'],
        hash: '77fdcef8759fb65da46a7a6431310829f12cef5b',
        isDot: false,
        isFollower: false,
        parents: ['3f41d88ab69446763404eccd0f3e579352ba2753'],
        grandparents: [],
        phase: 'draft',
        remoteBookmarks: [],
        stableCommitMetadata: undefined,
        successorInfo: undefined,
        title: '',
        totalFileCount: 1,
        maxCommonPathPrefix: 'sapling/addons/isl/',
      },
    ]);
  });
});

describe('max common path prefix', () => {
  it('extracts common prefix', () => {
    expect(findMaxCommonPathPrefix(['a/b/c', 'a/b/d', 'a/b/d/e'])).toEqual('a/b/');
    expect(
      findMaxCommonPathPrefix(['a/b/c1', 'a/b/c2', 'a/b/d/e/f', 'a/b/d/e/f/g/h/i', 'a/b/q']),
    ).toEqual('a/b/');
    expect(
      findMaxCommonPathPrefix([
        'addons/isl/src/file.ts',
        'addons/isl/README.md',
        'addons/isl/src/another.ts',
      ]),
    ).toEqual('addons/isl/');
  });
  it('handles root as common', () => {
    expect(findMaxCommonPathPrefix(['a/b/c', 'd/e/f'])).toEqual('');
    expect(findMaxCommonPathPrefix(['www/README', 'fbcode/foo'])).toEqual('');
    expect(findMaxCommonPathPrefix(['subdir/some/file', 'toplevel'])).toEqual('');
  });
  it('acts on full dir names', () => {
    expect(findMaxCommonPathPrefix(['a/foo1/a', 'a/foo2/b', 'a/foo3/c'])).toEqual(
      'a/', // not a/foo
    );
    expect(findMaxCommonPathPrefix(['foo/bananaspoon', 'foo/banana'])).toEqual(
      'foo/', // not foo/banana
    );
    expect(findMaxCommonPathPrefix(['foo/banana', 'foo/bananaspoon'])).toEqual(
      'foo/', // not foo/banana
    );
  });
  describe('windows', () => {
    const oldSep = path.sep;
    beforeEach(() => {
      (path.sep as string) = '\\';
    });
    afterEach(() => {
      (path.sep as string) = oldSep;
    });

    it('handles windows paths', () => {
      expect(
        findMaxCommonPathPrefix([
          'addons\\isl\\src\\file.ts',
          'addons\\isl\\README.md',
          'addons\\isl\\src\\another.ts',
        ]),
      ).toEqual('addons\\isl\\');
    });
  });
});
