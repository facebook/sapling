/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Rev} from '../fileStackState';
import type {ExportCommit, ExportStack} from 'shared/types/stack';

import {ABSENT_FILE, CommitIdx, CommitStackState, CommitState} from '../commitStackState';
import {FileStackState} from '../fileStackState';
import {List, Set as ImSet, Map as ImMap} from 'immutable';
import {unwrap} from 'shared/utils';

const exportCommitDefault: ExportCommit = {
  requested: true,
  immutable: false,
  author: 'test <test@example.com>',
  date: [0, 0],
  node: '',
  text: '',
};

// In this test we tend to use uppercase for commits (ex. A, B, C),
// and lowercase for files (ex. x, y, z).

/**
 * Created by `drawdag --no-files`:
 *
 *       C  # C/z.txt=(removed)
 *       |
 *       B  # B/y.txt=33 (renamed from x.txt)
 *       |
 *       A  # A/x.txt=33
 *       |  # A/z.txt=22
 *      /
 *     Z  # Z/z.txt=11
 *
 * and exported via `debugexportstack -r 'desc(A)::'`.
 */
const exportStack1: ExportStack = [
  {
    ...exportCommitDefault,
    immutable: true,
    node: 'Z_NODE',
    relevantFiles: {
      'x.txt': null,
      'z.txt': {data: '11'},
    },
    requested: false,
    text: 'Z',
  },
  {
    ...exportCommitDefault,
    files: {
      'x.txt': {data: '33'},
      'z.txt': {data: '22'},
    },
    node: 'A_NODE',
    parents: ['Z_NODE'],
    relevantFiles: {'y.txt': null},
    text: 'A',
  },
  {
    ...exportCommitDefault,
    files: {
      'x.txt': null,
      'y.txt': {copyFrom: 'x.txt', data: '33'},
    },
    node: 'B_NODE',
    parents: ['A_NODE'],
    relevantFiles: {'z.txt': {data: '22'}},
    text: 'B',
  },
  {
    ...exportCommitDefault,
    date: [0.0, 0],
    files: {'z.txt': null},
    node: 'C_NODE',
    parents: ['B_NODE'],
    text: 'C',
  },
];

describe('CommitStackState', () => {
  it('accepts an empty stack', () => {
    const stack = new CommitStackState([]);
    expect(stack.revs()).toStrictEqual([]);
  });

  it('accepts a stack without a public commit', () => {
    const stack = new CommitStackState([
      {
        ...exportCommitDefault,
        files: {'a.txt': {data: 'a'}},
        node: 'x',
        parents: [],
        text: 'A',
      },
    ]);
    expect(stack.revs()).toStrictEqual([0]);
  });

  it('rejects a stack with multiple roots', () => {
    const stack = [
      {...exportCommitDefault, node: 'Z1'},
      {...exportCommitDefault, node: 'Z2'},
    ];
    expect(() => new CommitStackState(stack)).toThrowError(
      'Multiple roots ["Z1","Z2"] is not supported',
    );
  });

  it('rejects a stack with merges', () => {
    const stack = [
      {...exportCommitDefault, node: 'A', parents: []},
      {...exportCommitDefault, node: 'B', parents: ['A']},
      {...exportCommitDefault, node: 'C', parents: ['A', 'B']},
    ];
    expect(() => new CommitStackState(stack)).toThrowError('Merge commit C is not supported');
  });

  it('rejects circular stack', () => {
    const stack = [
      {...exportCommitDefault, node: 'A', parents: ['B']},
      {...exportCommitDefault, node: 'B', parents: ['A']},
    ];
    expect(() => new CommitStackState(stack)).toThrowError();
  });

  it('provides file paths', () => {
    const stack = new CommitStackState(exportStack1);
    expect(stack.getAllPaths()).toStrictEqual(['x.txt', 'y.txt', 'z.txt']);
  });

  it('logs commit history', () => {
    const stack = new CommitStackState(exportStack1);
    expect(stack.revs()).toStrictEqual([0, 1, 2, 3]);
    expect([...stack.log(1)]).toStrictEqual([1, 0]);
    expect([...stack.log(3)]).toStrictEqual([3, 2, 1, 0]);
  });

  describe('log file history', () => {
    // [rev, path] => [rev, path, file]
    const extend = (stack: CommitStackState, revPathPairs: Array<[Rev, string]>) => {
      return revPathPairs.map(([rev, path]) => {
        const file = rev >= 0 ? stack.get(rev)?.files.get(path) : stack.bottomFiles.get(path);
        expect(file).toBe(stack.getFile(rev, path));
        return [rev, path, file];
      });
    };

    it('logs file history', () => {
      const stack = new CommitStackState(exportStack1);
      expect([...stack.logFile(3, 'x.txt')]).toStrictEqual(
        extend(stack, [
          [2, 'x.txt'],
          [1, 'x.txt'],
        ]),
      );
      expect([...stack.logFile(3, 'y.txt')]).toStrictEqual(extend(stack, [[2, 'y.txt']]));
      expect([...stack.logFile(3, 'z.txt')]).toStrictEqual(
        extend(stack, [
          [3, 'z.txt'],
          [1, 'z.txt'],
        ]),
      );
      // Changes in not requested commits (rev 0) are ignored.
      expect([...stack.logFile(3, 'k.txt')]).toStrictEqual([]);
    });

    it('logs file history following renames', () => {
      const stack = new CommitStackState(exportStack1);
      expect([...stack.logFile(3, 'y.txt', true)]).toStrictEqual(
        extend(stack, [
          [2, 'y.txt'],
          [1, 'x.txt'],
        ]),
      );
    });

    it('logs file history including the bottom', () => {
      const stack = new CommitStackState(exportStack1);
      ['x.txt', 'z.txt'].forEach(path => {
        expect([...stack.logFile(1, path, true, true)]).toStrictEqual(
          extend(stack, [
            [1, path],
            // rev 0 does not change x.txt or z.txt
            [-1, path],
          ]),
        );
      });
    });

    it('parentFile follows rename to bottomFile', () => {
      const stack = new CommitStackState([
        {
          ...exportCommitDefault,
          relevantFiles: {
            'x.txt': {data: '11'},
            'z.txt': {data: '22'},
          },
          files: {
            'z.txt': {data: '33', copyFrom: 'x.txt'},
          },
          text: 'Commit Foo',
        },
      ]);
      const file = stack.getFile(0, 'z.txt');
      expect(stack.getUtf8Data(file)).toBe('33');
      const [, , parentFileWithRename] = stack.parentFile(0, 'z.txt', true);
      expect(stack.getUtf8Data(parentFileWithRename)).toBe('11');
      const [, , parentFile] = stack.parentFile(0, 'z.txt', false);
      expect(stack.getUtf8Data(parentFile)).toBe('22');
    });
  });

  it('provides file contents at given revs', () => {
    const stack = new CommitStackState(exportStack1);
    expect(stack.getFile(0, 'x.txt')).toBe(ABSENT_FILE);
    expect(stack.getFile(0, 'y.txt')).toBe(ABSENT_FILE);
    expect(stack.getFile(0, 'z.txt')).toMatchObject({data: '11'});
    expect(stack.getFile(1, 'x.txt')).toMatchObject({data: '33'});
    expect(stack.getFile(1, 'y.txt')).toBe(ABSENT_FILE);
    expect(stack.getFile(1, 'z.txt')).toMatchObject({data: '22'});
    expect(stack.getFile(2, 'x.txt')).toBe(ABSENT_FILE);
    expect(stack.getFile(2, 'y.txt')).toMatchObject({data: '33'});
    expect(stack.getFile(2, 'z.txt')).toMatchObject({data: '22'});
    expect(stack.getFile(3, 'x.txt')).toBe(ABSENT_FILE);
    expect(stack.getFile(3, 'y.txt')).toMatchObject({data: '33'});
    expect(stack.getFile(3, 'z.txt')).toBe(ABSENT_FILE);
  });

  describe('builds FileStack', () => {
    it('for double renames', () => {
      // x.txt renamed to both y.txt and z.txt.
      const stack = new CommitStackState([
        {...exportCommitDefault, node: 'A', files: {'x.txt': {data: 'xx'}}},
        {
          ...exportCommitDefault,
          node: 'B',
          parents: ['A'],
          files: {
            'x.txt': null,
            'y.txt': {data: 'yy', copyFrom: 'x.txt'},
            'z.txt': {data: 'zz', copyFrom: 'x.txt'},
          },
        },
      ]);
      expect(stack.describeFileStacks()).toStrictEqual([
        // y.txt inherits x.txt's history.
        '0:./x.txt 1:A/x.txt(xx) 2:B/y.txt(yy)',
        // z.txt does not inherit x.txt's history (but still has a parent for diff rendering purpose).
        '0:A/x.txt(xx) 1:B/z.txt(zz)',
      ]);
    });

    it('for double copies', () => {
      // x.txt copied to both y.txt and z.txt.
      const stack = new CommitStackState([
        {...exportCommitDefault, node: 'A', files: {'x.txt': {data: 'xx'}}},
        {
          ...exportCommitDefault,
          node: 'B',
          parents: ['A'],
          files: {
            'y.txt': {data: 'yy', copyFrom: 'x.txt'},
            'z.txt': {data: 'zz', copyFrom: 'y.txt'},
          },
        },
      ]);
      expect(stack.describeFileStacks()).toStrictEqual([
        // y.txt connects to x.txt's history.
        '0:./x.txt 1:A/x.txt(xx) 2:B/y.txt(yy)',
        // z.txt does not connect to x.txt's history (but still have one parent for diff).
        '0:./y.txt 1:B/z.txt(zz)',
      ]);
    });

    it('for changes and copies', () => {
      // x.txt is changed, and copied to both y.txt and z.txt.
      const stack = new CommitStackState([
        {...exportCommitDefault, node: 'A', files: {'x.txt': {data: 'xx'}}},
        {
          ...exportCommitDefault,
          node: 'B',
          parents: ['A'],
          files: {
            'x.txt': {data: 'yy'},
            'y.txt': {data: 'xx', copyFrom: 'x.txt'},
            'z.txt': {data: 'xx', copyFrom: 'x.txt'},
          },
        },
      ]);
      expect(stack.describeFileStacks()).toStrictEqual([
        // x.txt has its own history.
        '0:./x.txt 1:A/x.txt(xx) 2:B/x.txt(yy)',
        // y.txt and z.txt do not share x.txt's history (but still have one parent for diff).
        '0:A/x.txt(xx) 1:B/y.txt(xx)',
        '0:A/x.txt(xx) 1:B/z.txt(xx)',
      ]);
    });

    it('for the the example stack', () => {
      const stack = new CommitStackState(exportStack1);
      expect(stack.describeFileStacks()).toStrictEqual([
        // x.txt: added by A, modified and renamed by B.
        '0:./x.txt 1:A/x.txt(33) 2:B/y.txt(33)',
        // z.txt: modified by A, deleted by C.
        '0:./z.txt(11) 1:A/z.txt(22) 2:C/z.txt',
      ]);
    });

    it('with rename tracking disabled', () => {
      const stack = new CommitStackState(exportStack1).buildFileStacks({followRenames: false});
      // no x.txt -> y.txt rename
      expect(stack.describeFileStacks()).toStrictEqual([
        '0:./x.txt 1:A/x.txt(33) 2:B/x.txt',
        '0:./z.txt(11) 1:A/z.txt(22) 2:C/z.txt',
        '0:./y.txt 1:B/y.txt(33)',
      ]);
    });
  });

  describe('calculates dependencies', () => {
    const e = exportCommitDefault;

    it('for content changes', () => {
      const stack = new CommitStackState([
        {...e, node: 'Z', requested: false, relevantFiles: {'x.txt': null}},
        {...e, node: 'A', parents: ['Z'], files: {'x.txt': {data: 'b\n'}}},
        {...e, node: 'B', parents: ['A'], files: {'x.txt': {data: 'a\nb\n'}}},
        {...e, node: 'C', parents: ['B'], files: {'x.txt': {data: 'a\nB\n'}}},
      ]);
      expect(stack.calculateDepMap()).toStrictEqual(
        new Map([
          [0, new Set()],
          [1, new Set()],
          [2, new Set()], // insertion at other insertion boundary is dependency-free
          [3, new Set([1])],
        ]),
      );
    });

    it('for file addition and deletion', () => {
      const stack = new CommitStackState([
        {...e, node: 'Z', requested: false, relevantFiles: {'x.txt': {data: 'a'}}},
        {...e, node: 'A', parents: ['Z'], files: {'x.txt': null}},
        {...e, node: 'B', parents: ['A'], files: {'x.txt': {data: 'a'}}},
        {...e, node: 'C', parents: ['B'], files: {'x.txt': null}},
      ]);
      expect(stack.calculateDepMap()).toStrictEqual(
        new Map([
          [0, new Set()],
          [1, new Set()],
          [2, new Set([1])], // commit B adds x.txt, depends on commit A's deletion.
          [3, new Set([2])], // commit C deletes x.txt, depends on commit B's addition.
        ]),
      );
    });

    it('for copies', () => {
      const stack = new CommitStackState([
        {...e, node: 'A', files: {'x.txt': {data: 'a'}}},
        {...e, node: 'B', parents: ['A'], files: {'y.txt': {data: 'a', copyFrom: 'x.txt'}}},
        {...e, node: 'C', parents: ['B'], files: {'z.txt': {data: 'a', copyFrom: 'x.txt'}}},
        {
          ...e,
          node: 'D',
          parents: ['C'],
          files: {'p.txt': {data: 'a', copyFrom: 'x.txt'}, 'q.txt': {data: 'a', copyFrom: 'z.txt'}},
        },
      ]);
      expect(stack.calculateDepMap()).toStrictEqual(
        new Map([
          [0, new Set()],
          [1, new Set([0])], // commit B copies commit A's x.txt to y.txt.
          [2, new Set([0])], // commit C copies commit A's x.txt to z.txt.
          [3, new Set([0, 2])], // commit D copies commit A's x.txt to p.txt, and commit C's z.txt to q.txt.
        ]),
      );
    });
  });

  describe('folding commits', () => {
    const e = exportCommitDefault;

    it('cannot be used for immutable commits', () => {
      const stack = new CommitStackState([
        {...e, node: 'A', immutable: true},
        {...e, node: 'B', parents: ['A'], immutable: false},
        {...e, node: 'C', parents: ['B'], immutable: false},
      ]);
      expect(stack.canFoldDown(1)).toBeFalsy();
      expect(stack.canFoldDown(2)).toBeTruthy();
    });

    it('cannot be used for out-of-range commits', () => {
      const stack = new CommitStackState([
        {...e, node: 'A'},
        {...e, node: 'B', parents: ['A']},
      ]);
      expect(stack.canFoldDown(0)).toBeFalsy();
      expect(stack.canFoldDown(1)).toBeTruthy();
      expect(stack.canFoldDown(2)).toBeFalsy();
    });

    it('cannot be used for forks', () => {
      const stack = new CommitStackState([
        {...e, node: 'A'},
        {...e, node: 'B', parents: ['A']},
        {...e, node: 'C', parents: ['A']},
      ]);
      expect(stack.canFoldDown(1)).toBeFalsy();
      expect(stack.canFoldDown(2)).toBeFalsy();
    });

    it('works for simple edits', () => {
      let stack = new CommitStackState([
        {
          ...e,
          node: 'A',
          text: 'Commit A',
          parents: [],
          files: {'x.txt': {data: 'xx'}, 'y.txt': {data: 'yy'}},
        },
        {...e, node: 'B', text: 'Commit B', parents: ['A'], files: {'x.txt': {data: 'yy'}}},
        {...e, node: 'C', text: 'Commit C', parents: ['B'], files: {'x.txt': {data: 'zz'}}},
      ]);
      expect(stack.canFoldDown(1)).toBeTruthy();
      stack = stack.foldDown(1);
      expect(stack.stack.size).toBe(2);
      expect(stack.stack.get(0)?.toJS()).toMatchObject({
        key: 'A',
        files: {
          'x.txt': {data: 'yy'},
          'y.txt': {data: 'yy'},
        },
        originalNodes: new Set(['A', 'B']),
        text: 'Commit A, Commit B',
        parents: [],
      });
      expect(stack.stack.get(1)?.toJS()).toMatchObject({
        key: 'C',
        text: 'Commit C',
        parents: [0], // Commit C's parent is updated to Commit A.
      });
    });

    it('removes copyFrom appropriately', () => {
      let stack = new CommitStackState([
        {...e, node: 'A', parents: [], files: {'x.txt': {data: 'xx'}}},
        {...e, node: 'B', parents: ['A'], files: {'y.txt': {data: 'yy', copyFrom: 'x.txt'}}},
      ]);
      expect(stack.canFoldDown(1)).toBeTruthy();
      stack = stack.foldDown(1);
      expect(stack.stack.get(0)?.toJS()).toMatchObject({
        files: {
          'x.txt': {data: 'xx'},
          'y.txt': {data: 'yy'}, // no longer has "copyFrom", since 'x.txt' does not exist in commit A.
        },
      });
    });

    it('keeps copyFrom appropriately', () => {
      let stack = new CommitStackState([
        {...e, node: 'A', parents: [], files: {xt: {data: 'xx'}, yt: {data: 'yy'}}},
        {...e, node: 'B', parents: ['A'], files: {y1t: {data: 'yy', copyFrom: 'yt'}}},
        {
          ...e,
          node: 'C',
          parents: ['B'],
          files: {x1t: {data: 'x1', copyFrom: 'xt'}, y1t: {data: 'y1'}},
        },
      ]);
      // Fold B+C.
      expect(stack.canFoldDown(2)).toBeTruthy();
      stack = stack.foldDown(2);
      expect(stack.stack.get(1)?.toJS()).toMatchObject({
        files: {
          y1t: {data: 'y1', copyFrom: 'yt'}, // reuse copyFrom: 'yt' from commit B.
          x1t: {data: 'x1', copyFrom: 'xt'}, // reuse copyFrom: 'xt' from commit C.
        },
      });
    });

    it('chains renames', () => {
      let stack = new CommitStackState([
        {...e, node: 'A', parents: [], files: {xt: {data: 'xx'}}},
        {...e, node: 'B', parents: ['A'], files: {yt: {data: 'yy', copyFrom: 'xt'}, xt: null}},
        {...e, node: 'C', parents: ['B'], files: {zt: {data: 'zz', copyFrom: 'yt'}, yt: null}},
      ]);
      // Fold B+C.
      expect(stack.canFoldDown(2)).toBeTruthy();
      stack = stack.foldDown(2);
      expect(stack.stack.get(1)?.toJS()).toMatchObject({
        files: {
          xt: ABSENT_FILE.toJS(),
          // 'yt' is no longer considered changed.
          zt: {data: 'zz', copyFrom: 'xt'}, // 'xt'->'yt'->'zt' is folded to 'xt'->'zt'.
        },
      });
    });

    it('removes cancel-out changes', () => {
      let stack = new CommitStackState([
        {...e, node: 'A', parents: [], files: {xt: {data: 'xx'}}},
        {...e, node: 'B', parents: ['A'], files: {xt: {data: 'yy'}, zt: {data: 'zz'}}},
        {...e, node: 'C', parents: ['B'], files: {xt: {data: 'xx'}}},
      ]);
      // Fold B+C.
      expect(stack.canFoldDown(2)).toBeTruthy();
      stack = stack.foldDown(2);
      expect(stack.stack.get(1)?.toJS()).toMatchObject({
        files: {zt: {data: 'zz'}}, // changes to 'yt' is removed.
      });
    });
  });

  describe('dropping commits', () => {
    const e = exportCommitDefault;

    it('cannot be used for immutable commits', () => {
      const stack = new CommitStackState([
        {...e, node: 'A', immutable: true},
        {...e, node: 'B', parents: ['A'], immutable: true},
        {...e, node: 'C', parents: ['B'], immutable: false},
      ]);
      expect(stack.canDrop(0)).toBeFalsy();
      expect(stack.canDrop(1)).toBeFalsy();
      expect(stack.canDrop(2)).toBeTruthy();
    });

    it('detects content dependencies', () => {
      const stack = new CommitStackState([
        {...e, node: 'A', files: {xx: {data: '0\n2\n'}}},
        {...e, node: 'B', parents: ['A'], files: {xx: {data: '0\n1\n2\n'}}},
        {...e, node: 'C', parents: ['B'], files: {xx: {data: '0\n1\n2\n3\n'}}},
        {...e, node: 'D', parents: ['C'], files: {xx: {data: '0\n1\n2\n4\n'}}},
      ]);
      expect(stack.canDrop(0)).toBeFalsy();
      expect(stack.canDrop(1)).toBeTruthy();
      expect(stack.canDrop(2)).toBeFalsy(); // D depends on C
      expect(stack.canDrop(3)).toBeTruthy();
    });

    it('detects commit graph dependencies', () => {
      const stack = new CommitStackState([
        {...e, node: 'A', files: {xx: {data: '1'}}},
        {...e, node: 'B', parents: ['A'], files: {xx: {data: '2'}}},
        {...e, node: 'C', parents: ['A'], files: {xx: {data: '3'}}},
        {...e, node: 'D', parents: ['C'], files: {xx: {data: '4'}}},
      ]);
      expect(stack.canDrop(0)).toBeFalsy();
      expect(stack.canDrop(1)).toBeTruthy();
      expect(stack.canDrop(2)).toBeFalsy();
      expect(stack.canDrop(3)).toBeTruthy();
    });

    it('for a change in the middle of a stack', () => {
      let stack = new CommitStackState([
        {...e, node: 'A', files: {xx: {data: 'p\ny\n'}}},
        {...e, node: 'B', parents: ['A'], files: {xx: {data: 'p\nx\ny\n'}}},
        {...e, node: 'C', parents: ['B'], files: {xx: {data: 'p\nx\ny\nz\n'}}},
      ]);
      expect(stack.canDrop(0)).toBeFalsy();
      expect(stack.canDrop(1)).toBeTruthy();
      expect(stack.canDrop(2)).toBeTruthy();
      stack = stack.drop(1);
      expect(stack.stack.size).toBe(2);
      expect(stack.stack.get(1)?.toJS()).toMatchObject({
        originalNodes: ['C'],
        files: {xx: {data: 'p\ny\nz\n'}},
      });
      expect(stack.stack.toArray().map(c => c.key)).toMatchObject(['A', 'C']);
    });
  });

  describe('reordering commits', () => {
    const e = exportCommitDefault;

    it('cannot be used for immutable commits', () => {
      const stack = new CommitStackState([
        {...e, node: 'A', immutable: true},
        {...e, node: 'B', parents: ['A'], immutable: true},
        {...e, node: 'C', parents: ['B'], immutable: false},
      ]);
      expect(stack.canReorder([0, 2, 1])).toBeFalsy();
      expect(stack.canReorder([1, 0, 2])).toBeFalsy();
      expect(stack.canReorder([0, 1, 2])).toBeTruthy();
    });

    it('respects content dependencies', () => {
      const stack = new CommitStackState([
        {...e, node: 'A', files: {xx: {data: '0\n2\n'}}},
        {...e, node: 'B', parents: ['A'], files: {xx: {data: '0\n1\n2\n'}}},
        {...e, node: 'C', parents: ['B'], files: {xx: {data: '0\n1\n2\n3\n'}}},
        {...e, node: 'D', parents: ['C'], files: {xx: {data: '0\n1\n2\n4\n'}}},
      ]);
      expect(stack.canReorder([0, 2, 3, 1])).toBeTruthy();
      expect(stack.canReorder([0, 2, 1, 3])).toBeTruthy();
      expect(stack.canReorder([0, 3, 2, 1])).toBeFalsy();
      expect(stack.canReorder([0, 3, 1, 2])).toBeFalsy();
    });

    it('refuses to reorder non-linear stack', () => {
      const stack = new CommitStackState([
        {...e, node: 'A', files: {xx: {data: '1'}}},
        {...e, node: 'B', parents: ['A'], files: {xx: {data: '2'}}},
        {...e, node: 'C', parents: ['A'], files: {xx: {data: '3'}}},
        {...e, node: 'D', parents: ['C'], files: {xx: {data: '4'}}},
      ]);
      expect(stack.canReorder([0, 2, 3, 1])).toBeFalsy();
      expect(stack.canReorder([0, 2, 1, 3])).toBeFalsy();
      expect(stack.canReorder([0, 1, 2, 3])).toBeFalsy();
    });

    it('can reorder a long stack', () => {
      const exportStack: ExportStack = [...Array(20).keys()].map(i => {
        return {...e, node: `A${i}`, parents: i === 0 ? [] : [`A${i - 1}`], files: {}};
      });
      const stack = new CommitStackState(exportStack);
      expect(stack.canReorder(stack.revs().reverse())).toBeTruthy();
    });

    it('reorders adjacent changes', () => {
      // Note: usually rev 0 is a public parent commit, rev 0 is not usually reordered.
      // But this test reorders rev 0 and triggers some interesting code paths.
      let stack = new CommitStackState([
        {...e, node: 'A', files: {xx: {data: '1\n'}}},
        {...e, node: 'B', parents: ['A'], files: {xx: {data: '1\n2\n'}}},
      ]);
      expect(stack.canReorder([1, 0])).toBeTruthy();
      stack = stack.reorder([1, 0]);
      expect(stack.stack.toArray().map(c => c.files.get('xx')?.data)).toMatchObject([
        '2\n',
        '1\n2\n',
      ]);
      expect(stack.stack.toArray().map(c => c.key)).toMatchObject(['B', 'A']);
      // Reorder back.
      expect(stack.canReorder([1, 0])).toBeTruthy();
      stack = stack.reorder([1, 0]);
      expect(stack.stack.toArray().map(c => c.files.get('xx')?.data)).toMatchObject([
        '1\n',
        '1\n2\n',
      ]);
      expect(stack.stack.toArray().map(c => c.key)).toMatchObject(['A', 'B']);
    });

    it('reorders content changes', () => {
      let stack = new CommitStackState([
        {...e, node: 'A', files: {xx: {data: '1\n1\n'}}},
        {...e, node: 'B', parents: ['A'], files: {xx: {data: '0\n1\n1\n'}}},
        {...e, node: 'C', parents: ['B'], files: {yy: {data: '0'}}}, // Does not change 'xx'.
        {...e, node: 'D', parents: ['C'], files: {xx: {data: '0\n1\n1\n2\n'}}},
        {...e, node: 'E', parents: ['D'], files: {xx: {data: '0\n1\n3\n1\n2\n'}}},
      ]);

      // A-B-C-D-E => A-C-E-B-D.
      let order = [0, 2, 4, 1, 3];
      expect(stack.canReorder(order)).toBeTruthy();
      stack = stack.reorder(order);
      const getNode = (r: Rev) => stack.stack.get(r)?.originalNodes?.first();
      const getParents = (r: Rev) => stack.stack.get(r)?.parents?.toJS();
      expect(stack.revs().map(getNode)).toMatchObject(['A', 'C', 'E', 'B', 'D']);
      expect(stack.revs().map(getParents)).toMatchObject([[], [0], [1], [2], [3]]);
      expect(stack.revs().map(r => stack.getFile(r, 'xx').data)).toMatchObject([
        '1\n1\n',
        '1\n1\n', // Not changed by 'C'.
        '1\n3\n1\n',
        '0\n1\n3\n1\n',
        '0\n1\n3\n1\n2\n',
      ]);
      expect(stack.revs().map(r => stack.getFile(r, 'yy').data)).toMatchObject([
        '',
        '0',
        '0',
        '0',
        '0',
      ]);

      // Reorder back. A-C-E-B-D => A-B-C-D-E.
      order = [0, 3, 1, 4, 2];
      expect(stack.canReorder(order)).toBeTruthy();
      stack = stack.reorder(order);
      expect(stack.revs().map(getNode)).toMatchObject(['A', 'B', 'C', 'D', 'E']);
      expect(stack.revs().map(getParents)).toMatchObject([[], [0], [1], [2], [3]]);
      expect(stack.revs().map(r => stack.getFile(r, 'xx').data)).toMatchObject([
        '1\n1\n',
        '0\n1\n1\n',
        '0\n1\n1\n',
        '0\n1\n1\n2\n',
        '0\n1\n3\n1\n2\n',
      ]);
    });
  });

  describe('calculating ImportStack', () => {
    it('skips all if nothing changed', () => {
      const stack = new CommitStackState(exportStack1);
      expect(stack.calculateImportStack()).toMatchObject([]);
    });

    it('skips unchanged commits', () => {
      // Edits B/y.txt, affects descendants C.
      const stack = new CommitStackState(exportStack1).updateEachFile((rev, file, path) =>
        path === 'y.txt' ? file.set('data', '333') : file,
      );
      expect(stack.calculateImportStack()).toMatchObject([
        [
          'commit',
          {
            mark: ':r2',
            date: [0, 0],
            text: 'B',
            parents: ['A_NODE'],
            predecessors: ['B_NODE'],
            files: {
              'x.txt': null,
              'y.txt': {data: '333', copyFrom: 'x.txt', flags: ''},
            },
          },
        ],
        [
          'commit',
          {
            mark: ':r3',
            date: [0, 0],
            text: 'C',
            parents: [':r2'],
            predecessors: ['C_NODE'],
            files: {'z.txt': null},
          },
        ],
      ]);
    });

    it('follows reorder', () => {
      // Reorder B and C in the example stack.
      const stack = new CommitStackState(exportStack1).reorder([0, 1, 3, 2]);
      expect(stack.calculateImportStack({goto: 'B_NODE', preserveDirtyFiles: true})).toMatchObject([
        ['commit', {text: 'C'}],
        ['commit', {mark: ':r3', text: 'B'}],
        ['reset', {mark: ':r3'}],
      ]);
    });

    it('stays at the stack top on reorder', () => {
      // Reorder B and C in the example stack.
      const stack = new CommitStackState(exportStack1).reorder([0, 1, 3, 2]);
      expect(stack.calculateImportStack({goto: 'C_NODE'})).toMatchObject([
        ['commit', {text: 'C'}],
        ['commit', {mark: ':r3', text: 'B'}],
        ['goto', {mark: ':r3'}],
      ]);
    });

    it('hides dropped commits', () => {
      let stack = new CommitStackState(exportStack1);
      const revs = stack.revs();
      // Drop the last 2 commits: B and C.
      stack = stack.drop(revs[revs.length - 1]).drop(revs[revs.length - 2]);
      expect(stack.calculateImportStack()).toMatchObject([
        [
          'hide',
          {
            nodes: ['B_NODE', 'C_NODE'],
          },
        ],
      ]);
    });

    it('produces goto or reset command', () => {
      const stack = new CommitStackState(exportStack1).updateEachFile((rev, file, path) =>
        path === 'y.txt' ? file.set('data', '333') : file,
      );
      expect(stack.calculateImportStack({goto: 3})).toMatchObject([
        ['commit', {}],
        ['commit', {}],
        ['goto', {mark: ':r3'}],
      ]);
      expect(stack.calculateImportStack({goto: 3, preserveDirtyFiles: true})).toMatchObject([
        ['commit', {}],
        ['commit', {}],
        ['reset', {mark: ':r3'}],
      ]);
    });

    it('optionally rewrites commit date', () => {
      // Swap the last 2 commits.
      const stack = new CommitStackState(exportStack1).reorder([0, 1, 3, 2]);
      expect(stack.calculateImportStack({rewriteDate: 40})).toMatchObject([
        ['commit', {date: [40, 0], text: 'C'}],
        ['commit', {date: [40, 0], text: 'B'}],
      ]);
    });
  });

  describe('denseSubStack', () => {
    it('provides bottomFiles', () => {
      const stack = new CommitStackState(exportStack1);
      let subStack = stack.denseSubStack(List([3])); // C
      // The bottom files contains z (deleted) and its content is before deletion.
      expect([...subStack.bottomFiles.keys()].sort()).toEqual(['z.txt']);
      expect(subStack.bottomFiles.get('z.txt')?.data).toBe('22');

      subStack = stack.denseSubStack(List([2, 3])); // B, C
      // The bottom files contains x (deleted), y (modified) and z (deleted).
      expect([...subStack.bottomFiles.keys()].sort()).toEqual(['x.txt', 'y.txt', 'z.txt']);
    });

    it('marks all files at every commit as changed', () => {
      const stack = new CommitStackState(exportStack1);
      const subStack = stack.denseSubStack(List([2, 3])); // B, C
      // All commits (B, C) should have 3 files (x.txt, y.txt, z.txt) marked as "changed".
      expect(subStack.stack.map(c => c.files.size).toJS()).toEqual([3, 3]);
      // All file stacks (x.txt, y.txt, z.txt) should have 3 revs (bottomFile, B, C).
      expect(subStack.fileStacks.map(f => f.revLength).toJS()).toEqual([3, 3, 3]);
    });
  });

  describe('insertEmpty', () => {
    const stack = new CommitStackState(exportStack1);
    const getRevs = (stack: CommitStackState) =>
      stack.stack.map(c => [c.rev, c.parents.toArray()]).toArray();

    it('updates revs of commits', () => {
      expect(getRevs(stack)).toEqual([
        [0, []],
        [1, [0]],
        [2, [1]],
        [3, [2]],
      ]);
      expect(getRevs(stack.insertEmpty(2, 'foo'))).toEqual([
        [0, []],
        [1, [0]],
        [2, [1]],
        [3, [2]],
        [4, [3]],
      ]);
    });

    it('inserts at stack top', () => {
      expect(getRevs(stack.insertEmpty(4, 'foo'))).toEqual([
        [0, []],
        [1, [0]],
        [2, [1]],
        [3, [2]],
        [4, [3]],
      ]);
    });

    it('uses the provided commit message', () => {
      const msg = 'provided message\nfoobar';
      [0, 2, 4].forEach(i => {
        expect(stack.insertEmpty(i, msg).stack.get(i)?.text).toBe(msg);
      });
    });

    it('provides unique keys for inserted commits', () => {
      const newStack = stack.insertEmpty(1, '').insertEmpty(1, '').insertEmpty(1, '');
      const keys = newStack.stack.map(c => c.key);
      expect(keys.size).toBe(ImSet(keys).size);
    });

    // The "originalNodes" are useful for split to set predecessors correctly.
    it('preserves the originalNodes with splitFromRev', () => {
      [1, 4].forEach(i => {
        const newStack = stack.insertEmpty(i, '', 2);
        expect(newStack.get(i)?.originalNodes).toBe(stack.get(2)?.originalNodes);
        expect(newStack.get(i)?.originalNodes?.isEmpty()).toBeFalsy();
        const anotherStack = stack.insertEmpty(i, '');
        expect(anotherStack.get(i)?.originalNodes?.isEmpty()).toBeTruthy();
      });
    });
  });

  describe('applySubStack', () => {
    const stack = new CommitStackState(exportStack1);
    const subStack = stack.denseSubStack(List([2, 3]));
    const emptyStack = subStack.set('stack', List());

    const getChangedFiles = (state: CommitStackState, rev: Rev): Array<string> => {
      return [...unwrap(state.stack.get(rev)).files.keys()].sort();
    };

    it('optimizes file changes by removing unmodified changes', () => {
      const newStack = stack.applySubStack(2, 4, subStack);
      expect(newStack.stack.size).toBe(stack.stack.size);
      // The original `stack` does not have unmodified changes.
      // To verify that `newStack` does not have unmodified changes, check it
      // against the original `stack`.
      stack.revs().forEach(i => {
        expect(getChangedFiles(newStack, i)).toEqual(getChangedFiles(stack, i));
      });
    });

    it('drops empty commits at the end of subStack', () => {
      // Change the 2nd commit in subStack to empty.
      const newSubStack = subStack.set(
        'stack',
        subStack.stack.setIn([1, 'files'], unwrap(subStack.stack.get(0)).files),
      );
      // `applySubStack` should drop the 2nd commit in `newSubStack`.
      const newStack = stack.applySubStack(2, 4, newSubStack);
      newStack.assertRevOrder();
      expect(newStack.stack.size).toBe(stack.stack.size - 1);
    });

    it('rewrites revs for the remaining of the stack', () => {
      const newStack = stack.applySubStack(1, 2, emptyStack);
      newStack.assertRevOrder();
      [1, 2].forEach(i => {
        expect(newStack.stack.get(i)?.toJS()).toMatchObject({rev: i, parents: [i - 1]});
      });
    });

    it('rewrites revs for the inserted stack', () => {
      const newStack = stack.applySubStack(2, 3, subStack);
      newStack.assertRevOrder();
      [2, 3, 4].forEach(i => {
        expect(newStack.stack.get(i)?.toJS()).toMatchObject({rev: i, parents: [i - 1]});
      });
    });

    it('preserves file contents of the old stack', () => {
      // Add a file 'x.txt' deleted by the original stack.
      const newSubStack = subStack.set(
        'stack',
        List([
          CommitState({
            key: 'foo',
            files: ImMap([['x.txt', stack.getFile(1, 'x.txt')]]),
          }),
        ]),
      );
      const newStack = stack.applySubStack(1, 3, newSubStack);

      // 'y.txt' was added by the old stack, not the new stack. So it is re-added
      // to preserve its old content.
      // 'x.txt' was added by the new stack, deleted by the old stack. So it is
      // re-deleted.
      expect(getChangedFiles(newStack, 2)).toEqual(['x.txt', 'y.txt', 'z.txt']);
      expect(newStack.getFile(2, 'y.txt').data).toBe('33');
      expect(newStack.getFile(2, 'x.txt')).toBe(ABSENT_FILE);
    });

    it('update keys to avoid conflict', () => {
      const oldKey = unwrap(stack.stack.get(1)).key;
      const newSubStack = subStack.set('stack', subStack.stack.setIn([0, 'key'], oldKey));
      const newStack = stack.applySubStack(2, 3, newSubStack);

      // Keys are still unique.
      const keys = newStack.stack.map(c => c.key);
      const keysSet = ImSet(keys);
      expect(keys.size).toBe(keysSet.size);
    });

    it('drops ABSENT flag if content is not empty', () => {
      // x.txt was deleted by subStack rev 0 (B). We are moving it to be deleted by rev 1 (C).
      expect(subStack.getFile(0, 'x.txt').flags).toBe(ABSENT_FILE.flags);
      // To break the deletion into done by 2 commits, we edit the file stack of 'x.txt'.
      const fileIdx = unwrap(subStack.commitToFile.get(CommitIdx({rev: 0, path: 'x.txt'}))).fileIdx;
      const fileStack = unwrap(subStack.fileStacks.get(fileIdx));
      // The file stack has 3 revs: (base, before deletion), (deleted at rev 0), (deleted at rev 1).
      expect(fileStack.convertToPlainText().toArray()).toEqual(['33', '', '']);
      const newFileStack = new FileStackState(['33', '3', '']);
      const newSubStack = subStack.setFileStack(fileIdx, newFileStack);
      expect(newSubStack.getUtf8Data(newSubStack.getFile(0, 'x.txt'))).toBe('3');
      // Apply the file stack back to the main stack.
      const newStack = stack.applySubStack(2, 4, newSubStack);
      expect(newStack.stack.size).toBe(4);
      // Check that x.txt in rev 2 (B) is '3', not absent.
      const file = newStack.getFile(2, 'x.txt');
      expect(file.data).toBe('3');
      expect(file.flags ?? '').not.toContain(ABSENT_FILE.flags);

      // Compare the old and new file stacks.
      // - x.txt deletion is now by commit 'C', not 'B'.
      // - x.txt -> y.txt rename is preserved.
      expect(stack.describeFileStacks(true)).toEqual([
        '0:./x.txt 1:A/x.txt(33) 2:B/y.txt(33)',
        '0:./z.txt(11) 1:A/z.txt(22) 2:C/z.txt',
      ]);
      expect(newStack.describeFileStacks(true)).toEqual([
        '0:./x.txt 1:A/x.txt(33) 2:B/x.txt(3) 3:C/x.txt',
        '0:./z.txt(11) 1:A/z.txt(22) 2:C/z.txt',
        '0:A/x.txt(33) 1:B/y.txt(33)',
      ]);
    });

    it('does not add ABSENT flag if content becomes empty', () => {
      // This was a herustics when `flags` are not handled properly. Now it is no longer needed.
      // y.txt was added by subStack rev 0 (B). We are moving it to be added by rev 1 (C).
      const fileIdx = unwrap(subStack.commitToFile.get(CommitIdx({rev: 0, path: 'y.txt'}))).fileIdx;
      const fileStack = unwrap(subStack.fileStacks.get(fileIdx));
      // The file stack has 3 revs: (base, before add), (add by rev 0), (unchanged by rev 1).
      expect(fileStack.convertToPlainText().toArray()).toEqual(['', '33', '33']);
      const newFileStack = new FileStackState(['', '', '33']);
      const newSubStack = subStack.setFileStack(fileIdx, newFileStack);
      // Apply the file stack back to the main stack.
      const newStack = stack.applySubStack(2, 4, newSubStack);
      // Check that y.txt in rev 2 (B) is absent, not just empty.
      const file = newStack.getFile(2, 'y.txt');
      expect(file.data).toBe('');
      expect(file.flags).toBe('');
    });
  });
});
