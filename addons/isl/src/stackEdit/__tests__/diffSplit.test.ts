/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitRev} from '../common';

import {ABSENT_FLAG} from '../common';
import {applyDiffSplit, diffCommit, displayDiff} from '../diffSplit';
import {linearStackWithFiles} from './commitStackState.test';

describe('diffCommit', () => {
  const stack = linearStackWithFiles([
    {'x.txt': {dataBase85: 'HHa&FWG5;0PL2'}, 'z.txt': {data: ''}},
    {'x.txt': {data: '1\n2\n3\n4\n'}},
    {'x.txt': null, 'y.txt': {copyFrom: 'x.txt', data: '3\n4\n5\n6', flags: 'x'}},
    {'z.txt': null},
  ]);

  it('exports commit diff as json', () => {
    // Commit 2 includes deletion of "x.txt", and rename from "x.txt" to "y.txt".
    // Right now we don't "optimize" the diff to exclude the deletion.
    const diff = diffCommit(stack, 2 as CommitRev);
    expect(diff).toMatchInlineSnapshot(`
      {
        "files": [
          {
            "aFlag": "",
            "aPath": "x.txt",
            "bFlag": "a",
            "bPath": "x.txt",
            "lines": [
              {
                "a": 0,
                "b": null,
                "content": "1
      ",
              },
              {
                "a": 1,
                "b": null,
                "content": "2
      ",
              },
              {
                "a": 2,
                "b": null,
                "content": "3
      ",
              },
              {
                "a": 3,
                "b": null,
                "content": "4
      ",
              },
            ],
          },
          {
            "aFlag": "",
            "aPath": "x.txt",
            "bFlag": "x",
            "bPath": "y.txt",
            "lines": [
              {
                "a": 0,
                "b": null,
                "content": "1
      ",
              },
              {
                "a": 1,
                "b": null,
                "content": "2
      ",
              },
              {
                "a": 2,
                "b": 0,
                "content": "3
      ",
              },
              {
                "a": 3,
                "b": 1,
                "content": "4
      ",
              },
              {
                "a": null,
                "b": 2,
                "content": "5
      ",
              },
              {
                "a": null,
                "b": 3,
                "content": "6",
              },
            ],
          },
        ],
        "message": "Commit 2",
      }
    `);
    expect(displayDiff(diff)).toMatchInlineSnapshot(`
      "Commit 2
      diff a/x.txt b/x.txt
      deleted file mode 100644
      -1
      -2
      -3
      -4
      diff a/x.txt b/y.txt
      old mode 100644
      new mode 100755
      copy from x.txt
      copy to y.txt
      -1
      -2
       3
       4
      +5
      +6
      \\ No newline at end of file"
    `);
  });

  it('skips binary changes', () => {
    // Commit 1 modifies "x.txt" from binary to text. It is skipped because the binary data.
    const diff = diffCommit(stack, 1 as CommitRev);
    expect(diff).toMatchInlineSnapshot(`
      {
        "files": [],
        "message": "Commit 1",
      }
    `);
    expect(displayDiff(diff)).toMatchInlineSnapshot(`
      "Commit 1
      "
    `);
  });

  it('reports deletion of an empty file', () => {
    // Commit 3 deletes "z.txt". It should be reported despite the content diff is empty.
    const diff = diffCommit(stack, 3 as CommitRev);
    expect(diff).toMatchInlineSnapshot(`
      {
        "files": [
          {
            "aFlag": "",
            "aPath": "z.txt",
            "bFlag": "a",
            "bPath": "z.txt",
            "lines": [],
          },
        ],
        "message": "Commit 3",
      }
    `);
    expect(displayDiff(diff)).toMatchInlineSnapshot(`
      "Commit 3
      diff a/z.txt b/z.txt
      deleted file mode 100644
      "
    `);
  });
});

describe('applyDiffSplit', () => {
  it('works in a basic case', () => {
    const stack = linearStackWithFiles([
      {'x.txt': {data: '', flags: ABSENT_FLAG}},
      {'x.txt': {data: '1\n2\n3\n4\n'}},
      {'x.txt': {data: '3\n4\n5\n6\n'}},
      {'x.txt': {data: '3\n4\n5\n6\n7\n'}},
    ]);
    const diff = diffCommit(stack, 2 as CommitRev);
    expect(diff.files[0].lines).toMatchInlineSnapshot(`
      [
        {
          "a": 0,
          "b": null,
          "content": "1
      ",
        },
        {
          "a": 1,
          "b": null,
          "content": "2
      ",
        },
        {
          "a": 2,
          "b": 0,
          "content": "3
      ",
        },
        {
          "a": 3,
          "b": 1,
          "content": "4
      ",
        },
        {
          "a": null,
          "b": 2,
          "content": "5
      ",
        },
        {
          "a": null,
          "b": 3,
          "content": "6
      ",
        },
      ]
    `);
    expect(displayDiff(diff)).toMatchInlineSnapshot(`
      "Commit 2
      diff a/x.txt b/x.txt
      -1
      -2
       3
       4
      +5
      +6
      "
    `);

    // Pick "-2" and "+5" in the first commit, "+6" in the 2nd, and does not specify the 3rd.
    const newStack = applyDiffSplit(stack, 2 as CommitRev, [
      {message: 'Commit 2a', files: [{bPath: 'x.txt', aLines: [1], bLines: [2]}]},
      {message: 'Commit 2b', files: [{bPath: 'x.txt', aLines: [], bLines: [3]}]},
      {message: 'Commit 2c', files: []},
    ]);
    expect(
      [0, 1, 2, 3, 4, 5].map(i => displayDiff(diffCommit(newStack, i as CommitRev))).join('\n'),
    ).toMatchInlineSnapshot(`
      "Commit 0

      Commit 1
      diff a/x.txt b/x.txt
      new file mode 100644
      +1
      +2
      +3
      +4

      Commit 2a
      diff a/x.txt b/x.txt
       1
      -2
       3
       4
      +5

      Commit 2b
      diff a/x.txt b/x.txt
       1
       3
       4
       5
      +6

      Commit 2c
      diff a/x.txt b/x.txt
      -1
       3
       4
       5
       6

      Commit 3
      diff a/x.txt b/x.txt
       3
       4
       5
       6
      +7
      "
    `);
  });
});
