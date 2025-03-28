/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitRev} from '../common';

import {CommitStackState} from '../commitStackState';
import {diffCommit, displayDiff} from '../diffSplit';
import {exportCommitDefault} from './commitStackState.test';

describe('diffCommit', () => {
  const stack = new CommitStackState([
    {
      ...exportCommitDefault,
      immutable: true,
      node: 'Z_NODE',
      relevantFiles: {'x.txt': {dataBase85: 'HHa&FWG5;0PL2'}, 'z.txt': {data: ''}},
      requested: false,
      text: 'Z',
    },
    {
      ...exportCommitDefault,
      files: {'x.txt': {data: '1\n2\n3\n4\n'}},
      node: 'A_NODE',
      parents: ['Z_NODE'],
      relevantFiles: {'y.txt': null},
      text: 'A',
    },
    {
      ...exportCommitDefault,
      files: {
        'x.txt': null,
        'y.txt': {copyFrom: 'x.txt', data: '3\n4\n5\n6', flags: 'x'},
      },
      node: 'B_NODE',
      parents: ['A_NODE'],
      text: 'B',
    },
    {
      ...exportCommitDefault,
      files: {'z.txt': null},
      node: 'C_NODE',
      parents: ['Z_NODE'],
      text: 'C',
    },
  ]);

  it('exports commit diff as json', () => {
    // Commit "B" includes deletion of "x.txt", and rename from "x.txt" to "y.txt".
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
        "message": "B",
      }
    `);
    expect(displayDiff(diff)).toMatchInlineSnapshot(`
      "B
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
    // Commit "A" modifies "x.txt" from binary to text. It is skipped because the binary data.
    const diff = diffCommit(stack, 1 as CommitRev);
    expect(diff).toMatchInlineSnapshot(`
        {
          "files": [],
          "message": "A",
        }
      `);
    expect(displayDiff(diff)).toMatchInlineSnapshot(`
      "A
      "
    `);
  });

  it('reports deletion of an empty file', () => {
    // Commit "C" deletes "z.txt". It should be reported despite the content diff is empty.
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
        "message": "C",
      }
    `);
    expect(displayDiff(diff)).toMatchInlineSnapshot(`
      "C
      diff a/z.txt b/z.txt
      deleted file mode 100644
      "
    `);
  });
});
