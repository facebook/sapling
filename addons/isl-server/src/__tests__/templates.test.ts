/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ChangedFile} from 'isl/src/types';

import {COMMIT_END_MARK, findMaxCommonPathPrefix, parseCommitInfoOutput} from '../templates';
import path from 'path';
import {mockLogger} from 'shared/testUtils';

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

[]
["sapling/addons/isl/README.md"]
[]


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

["sapling/addons/isl/README.md"]
[]
[]

9637166dabea9ac50ccb93902f3f41df4d8a15c4,

false
|
Commit B
`,
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
        filesSample: [
          {
            path: 'sapling/addons/isl/README.md',
            status: 'M',
          },
        ],
        hash: '77fdcef8759fb65da46a7a6431310829f12cef5b',
        isDot: false,
        isFollower: false,
        parents: ['3f41d88ab69446763404eccd0f3e579352ba2753'],
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
        filesSample: [
          {
            path: 'sapling/addons/isl/README.md',
            status: 'A',
          },
        ],
        hash: 'e4594714fb9b3410a0ef4affc955f9d76d61c8a7',
        isDot: false,
        isFollower: false,
        parents: ['2934650733c9181bdf64b7d00f5e0c7ca93d7ed7'],
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

[]
["sapling/addons/isl/README.md"]
[]


1
false
|
${COMMIT_END_MARK}
`,
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
        filesSample: [
          {
            path: 'sapling/addons/isl/README.md',
            status: 'M',
          },
        ],
        hash: '77fdcef8759fb65da46a7a6431310829f12cef5b',
        isDot: false,
        isFollower: false,
        parents: ['3f41d88ab69446763404eccd0f3e579352ba2753'],
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
  const FILE = (path: string): ChangedFile => ({
    path,
    status: 'M',
  });
  it('extracts common prefix', () => {
    expect(findMaxCommonPathPrefix([FILE('a/b/c'), FILE('a/b/d'), FILE('a/b/d/e')])).toEqual(
      'a/b/',
    );
    expect(
      findMaxCommonPathPrefix([
        FILE('a/b/c1'),
        FILE('a/b/c2'),
        FILE('a/b/d/e/f'),
        FILE('a/b/d/e/f/g/h/i'),
        FILE('a/b/q'),
      ]),
    ).toEqual('a/b/');
    expect(
      findMaxCommonPathPrefix([
        FILE('addons/isl/src/file.ts'),
        FILE('addons/isl/README.md'),
        FILE('addons/isl/src/another.ts'),
      ]),
    ).toEqual('addons/isl/');
  });
  it('handles root as common', () => {
    expect(findMaxCommonPathPrefix([FILE('a/b/c'), FILE('d/e/f')])).toEqual('');
    expect(findMaxCommonPathPrefix([FILE('www/README'), FILE('fbcode/foo')])).toEqual('');
    expect(findMaxCommonPathPrefix([FILE('subdir/some/file'), FILE('toplevel')])).toEqual('');
  });
  it('acts on full dir names', () => {
    expect(findMaxCommonPathPrefix([FILE('a/foo1/a'), FILE('a/foo2/b'), FILE('a/foo3/c')])).toEqual(
      'a/', // not a/foo
    );
    expect(findMaxCommonPathPrefix([FILE('foo/bananaspoon'), FILE('foo/banana')])).toEqual(
      'foo/', // not foo/banana
    );
    expect(findMaxCommonPathPrefix([FILE('foo/banana'), FILE('foo/bananaspoon')])).toEqual(
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
          FILE('addons\\isl\\src\\file.ts'),
          FILE('addons\\isl\\README.md'),
          FILE('addons\\isl\\src\\another.ts'),
        ]),
      ).toEqual('addons\\isl\\');
    });
  });
});
