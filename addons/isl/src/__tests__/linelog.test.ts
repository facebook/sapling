/**
 * Portions Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/*

Copyright (c) 2020 Jun Wu

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

*/

import type {Rev} from '../linelog';

import {LineLog} from '../linelog';
import {describe, it, expect} from '@jest/globals';

describe('LineLog', () => {
  it('can be empty', () => {
    const log = new LineLog();
    expect(log.maxRev).toBe(0);
    expect(log.content).toBe('');
  });

  it('supports a single edit', () => {
    const log = new LineLog();
    log.recordText('c\nd\ne');
    expect(log.maxRev).toBe(1);
    expect(log.content).toBe('c\nd\ne');

    expect(log.checkOutLines(1)).toMatchObject([
      {data: 'c\n', rev: 1},
      {data: 'd\n', rev: 1},
      {data: 'e', rev: 1},
      {data: '', rev: 0},
    ]);
  });

  it('supports modifying rev 0', () => {
    const log = new LineLog();
    log.recordText('c\n', 0);
    expect(log.maxRev).toBe(0);
    expect(log.content).toBe('c\n');
    expect(log.checkOutLines(0)[0]).toMatchObject({rev: 0});
    log.recordText('c\nd', 1);
    expect(log.checkOutLines(1)[1]).toMatchObject({rev: 1});
    log.checkOut(0);
    expect(log.content).toBe('c\n');
    expect(log.checkOutLines(0)[0]).toMatchObject({rev: 0});
  });

  it('supports multiple edits', () => {
    const log = new LineLog();
    log.recordText('c\nd\ne\n');
    log.recordText('d\ne\nf\n');
    expect(log.maxRev).toBe(2);
    expect(log.content).toBe('d\ne\nf\n');
    expect(log.checkOutLines(2)).toMatchObject([
      {data: 'd\n', rev: 1, deleted: false},
      {data: 'e\n', rev: 1, deleted: false},
      {data: 'f\n', rev: 2, deleted: false},
      {data: '', rev: 0, deleted: false},
    ]);
  });

  it('supports checkout', () => {
    const log = new LineLog();
    log.recordText('c\nd\ne\n');
    log.recordText('d\ne\nf\n');
    log.checkOut(1);
    expect(log.content).toBe('c\nd\ne\n');
    log.checkOut(0);
    expect(log.lines[0].deleted).toBe(false);
    expect(log.content).toBe('');
    expect(log.checkOutLines(0)).toMatchObject([{data: ''}]);
    log.checkOut(2);
    expect(log.content).toBe('d\ne\nf\n');
    expect(log.lines[2].deleted).toBe(false);
  });

  it('supports checkout range', () => {
    const log = new LineLog();
    log.recordText('c\nd\ne\n'); // rev 1
    log.recordText('d\ne\nf\n'); // rev 2
    log.recordText('e\ng\nf\n'); // rev 3

    expect(log.checkOutLines(2, 1)).toMatchObject([
      {data: 'c\n', rev: 1, deleted: true}, // 'c' not in rev 2
      {data: 'd\n', rev: 1, deleted: false},
      {data: 'e\n', rev: 1, deleted: false},
      {data: 'f\n', rev: 2, deleted: false},
      {data: '', rev: 0, deleted: false}, // END
    ]);

    expect(log.checkOutLines(3, 0)).toMatchObject([
      {data: 'c\n', rev: 1, deleted: true}, // 'c' not in rev 3
      {data: 'd\n', rev: 1, deleted: true}, // 'd' not in rev 3
      {data: 'e\n', rev: 1, deleted: false}, // 'e' in rev 3
      {data: 'g\n', rev: 3, deleted: false},
      {data: 'f\n', rev: 2, deleted: false},
      {data: '', rev: 0, deleted: false},
    ]);

    // should not reuse cache
    expect(log.checkOut(3)).toBe('e\ng\nf\n');

    expect(log.checkOutLines(3, 2)).toMatchObject([
      {data: 'd\n', rev: 1, deleted: true},
      {data: 'e\n', rev: 1, deleted: false},
      {data: 'g\n', rev: 3, deleted: false},
      {data: 'f\n', rev: 2, deleted: false},
      {data: ''},
    ]);
  });

  it('bumps rev when recording the same content', () => {
    const log = new LineLog();
    expect(log.recordText('a\n')).toBe(1);
    expect(log.recordText('a\n')).toBe(2);
    expect(log.recordText('a\n')).toBe(3);
  });

  describe('supports editing previous revisions', () => {
    it('edits stack bottom', () => {
      const textList = ['a\n', 'a\nb\n', 'z\na\nb\n'];
      const log = logFromTextList(textList);

      expect(log.recordText('1\n2\n', 1)).toBe(1); // replace rev 1 from "a" to "1 2"
      expect(log.checkOut(1)).toBe('1\n2\n');
      expect(log.checkOut(2)).toBe('1\n2\nb\n');
      expect(log.checkOut(3)).toBe('z\n1\n2\nb\n');

      expect(log.recordText('', 1)).toBe(1); // replace rev 1 to ""
      expect(log.checkOut(1)).toBe('');
      expect(log.checkOut(2)).toBe('b\n');
      expect(log.checkOut(3)).toBe('z\nb\n');
    });

    it('edits stack middle', () => {
      const textList = ['c\nd\ne\n', 'b\nc\nd\n', 'a\nb\nc\nz\n'];
      let log = logFromTextList(textList);

      expect(log.recordText('b\nd\n', 2)).toBe(2); // remove "c" from "b c d" in rev 2
      expect(log.checkOut(1)).toBe('c\nd\ne\n'); // rev 1 is unchanged, despite "c" comes from rev 1
      expect(log.checkOut(2)).toBe('b\nd\n');
      expect(log.checkOut(3)).toBe('a\nb\nz\n'); // "c" in rev 3 is also removed

      log = logFromTextList(textList);
      log.recordText('b\nc\ny\ny\n', 2); // change "d" to "y y" from rev 2.
      expect(log.checkOut(3)).toBe('a\nb\nc\nz\n'); // rev 3 is unchanged, since "d" was deleted

      log = logFromTextList(textList);
      log.recordText('k\n', 2); // replace rev 2 with "k", this is a tricky case
      expect(log.checkOut(3)).toBe('a\nk\n'); // "a k" is the current implementation, "a k z" might be better
    });
  });

  it('calculates rev dependencies', () => {
    const textList = [
      'a\nb\nc\n',
      'a\nb\nc\nd\n',
      'z\na\nb\nc\nd\n',
      'z\na\nd\n',
      'a\nd\n',
      'a\nd\ne\nf\n',
      'a\nd\ne\n',
      'a\nd\n1\ne\n',
      'x\ny\nz\n',
    ];
    const log = logFromTextList(textList);
    const flatten = (depMap: Map<Rev, Set<Rev>>) =>
      [...depMap.entries()].map(([rev, set]) => [rev, [...set].sort()]);
    expect(flatten(log.calculateDepMap())).toStrictEqual([
      [1, [0]],
      [2, [0, 1]],
      [3, [1]],
      // deletes "c" added by rev 2
      [4, [1, 2]],
      // deletes "z" added by rev 3
      [5, [1, 3]],
      // appends after "d" added by rev 2
      [6, [0, 2]],
      // deletes "f" added by rev 6
      [7, [0, 6]],
      // inserts "1" between "d" (rev 2) and "e" (rev 6)
      [8, [2, 6]],
      // replaces all: "a" (rev 1), "d" (rev 2), "1" (rev 8), "e" (rev 6)
      [9, [0, 1, 2, 6, 8]],
    ]);
  });

  it('produces flatten lines', () => {
    const textList = ['a\nb\nc\n', 'b\nc\nd\ne\n', 'a\nc\nd\nf\n'];
    const log = logFromTextList(textList);
    const lines = log.flatten();
    expect(lines).toEqual(
      [
        ['a', [1]],
        ['a', [3]],
        ['b', [1, 2]],
        ['c', [1, 2, 3]],
        ['d', [2, 3]],
        ['f', [3]],
        ['e', [2]],
      ].map(([line, revs]) => ({revs: new Set(revs as number[]), data: `${line}\n`})),
    );
    // Verify the flatten lines against definition - if "revs" contains the rev,
    // then the line is included in "rev".
    for (let rev = 1; rev <= textList.length; rev++) {
      const text = lines
        .filter(line => line.revs.has(rev))
        .map(line => line.data)
        .join('');
      expect(text).toBe(textList[rev - 1]);
    }
  });

  describe('supports remapping revisions', () => {
    it('updates maxRev up', () => {
      const log = logFromTextList(['a', 'b']);
      log.remapRevs(new Map([[1, 10]]));
      expect(log.maxRev).toBe(10);
    });

    it('updates maxRev down', () => {
      const log = new LineLog();
      log.recordText('a\n', 10);
      log.remapRevs(new Map([[10, 5]]));
      expect(log.maxRev).toBe(5);
    });

    it('invalidates previous checkout', () => {
      const log = logFromTextList(['b\n', 'b\nc\n', 'a\nb\nc\n']);
      log.checkOut(2);
      log.remapRevs(
        new Map([
          [2, 3],
          [3, 2],
        ]),
      );
      expect(log.content).not.toBe('b\nc\n');
    });

    it('can reorder changes', () => {
      const log = logFromTextList(['b\n', 'b\nc\n', 'a\nb\nc\n']);
      log.remapRevs(
        new Map([
          [2, 3],
          [3, 2],
        ]),
      );
      expect(log.checkOut(1)).toBe('b\n');
      expect(log.checkOut(2)).toBe('a\nb\n');
      expect(log.checkOut(3)).toBe('a\nb\nc\n');
      expect(log.checkOutLines(3)).toMatchObject([
        {data: 'a\n', rev: 2},
        {data: 'b\n', rev: 1},
        {data: 'c\n', rev: 3},
        {data: '', rev: 0},
      ]);
    });

    it('can merge changes', () => {
      const log = logFromTextList(['b\n', 'b\nc\n', 'a\nb\nc\n']);
      log.remapRevs(new Map([[2, 1]]));
      expect(log.checkOut(1)).toBe('b\nc\n');
      expect(log.checkOut(2)).toBe('b\nc\n');
      expect(log.checkOut(3)).toBe('a\nb\nc\n');
    });

    it('can insert changes', () => {
      const log = logFromTextList(['b\n', 'b\nc\n']);
      log.remapRevs(new Map([[2, 3]]));
      log.recordText('a\nb\n', 2);
      expect(log.checkOut(3)).toBe('a\nb\nc\n');
    });

    it('does not check dependencies or conflicts', () => {
      // rev 2: +b between a and c. rev 2 depends on rev 1.
      const log = logFromTextList(['a\nc\n', 'a\nb\nc\n']);
      log.remapRevs(
        new Map([
          [1, 2],
          [2, 1],
        ]),
      );
      // rev 1 is now empty, not 'b'.
      expect(log.checkOut(1)).toBe('');
      expect(log.checkOut(2)).toBe('a\nb\nc\n');
    });
  });
});

function logFromTextList(textList: string[]): LineLog {
  const log = new LineLog();
  textList.forEach(text => log.recordText(text));
  return log;
}
