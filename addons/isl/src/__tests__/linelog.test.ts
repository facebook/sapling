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
    expect(log.getLineRev(0)).toBe(1);
    expect(log.getLineRev(1)).toBe(1);
    expect(log.getLineRev(2)).toBe(1);
    expect(log.getLineRev(3)).toBeNull(); // out of range
    expect(log.lines[0].deleted).toBe(false);
  });

  it('supports multiple edits', () => {
    const log = new LineLog();
    log.recordText('c\nd\ne\n');
    log.recordText('d\ne\nf\n');
    expect(log.maxRev).toBe(2);
    expect(log.content).toBe('d\ne\nf\n');
    expect(log.getLineRev(0)).toBe(1);
    expect(log.getLineRev(1)).toBe(1);
    expect(log.getLineRev(2)).toBe(2);
    expect(log.getLineRev(3)).toBeNull(); // out of range
    expect(log.lines[0].deleted).toBe(false);
    expect(log.lines[2].deleted).toBe(false);
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
    expect(log.getLineRev(0)).toBeNull();
    log.checkOut(2);
    expect(log.content).toBe('d\ne\nf\n');
    expect(log.lines[2].deleted).toBe(false);
  });

  it('supports checkout range', () => {
    const log = new LineLog();
    log.recordText('c\nd\ne\n'); // rev 1
    log.recordText('d\ne\nf\n'); // rev 2
    log.recordText('e\ng\nf\n'); // rev 3

    log.checkOut(2, 1);
    expect(log.content).toBe('c\nd\ne\nf\n');
    expect(log.lines[0].deleted).toBeTruthy(); // 'c' not in rev 2
    expect(!log.lines[1].deleted).toBeTruthy(); // 'd' in rev 2
    expect(!log.lines[2].deleted).toBeTruthy();
    expect(!log.lines[3].deleted).toBeTruthy();

    log.checkOut(3, 0);
    expect(log.content).toBe('c\nd\ne\ng\nf\n');
    expect(log.lines[0].deleted).toBeTruthy(); // 'c' not in rev 3
    expect(log.lines[1].deleted).toBeTruthy(); // 'd' not in rev 3
    expect(!log.lines[2].deleted).toBeTruthy(); // 'e' in rev 3

    log.checkOut(3); // should not reuse cache
    expect(log.content).toBe('e\ng\nf\n');

    log.checkOut(3, 2);
    expect(log.content).toBe('d\ne\ng\nf\n');
    expect(log.lines[0].deleted).toBeTruthy(); // 'd' not in rev 3
    expect(!log.lines[1].deleted).toBeTruthy(); // 'e' in rev 3
    expect(!log.lines[3].deleted).toBeTruthy(); // 'f' in rev 3
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

  it('tracks rev dependencies', () => {
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
    let log = logFromTextList(textList, {trackDeps: true});
    expect(log.revDepMap).toEqual(
      new Map([
        [1, new Set([])],
        [2, new Set([1])],
        [3, new Set([1])],
        // deletes "c" added by rev 2
        [4, new Set([1, 2])],
        // deletes "z" added by rev 3
        [5, new Set([1, 3])],
        // appends after "d" added by rev 2
        [6, new Set([2])],
        // deletes "f" added by rev 6
        [7, new Set([6])],
        // inserts "1" between "d" (rev 2) and "e" (rev 6)
        [8, new Set([2, 6])],
        // replaces all: "a" (rev 1), "d" (rev 2), "1" (rev 8), "e" (rev 6)
        [9, new Set([1, 2, 8, 6])],
      ]),
    );

    // Disable trackDeps
    log = logFromTextList(textList, {trackDeps: false});
    expect(log.revDepMap).toEqual(new Map());
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
});

function logFromTextList(textList: string[], {trackDeps} = {trackDeps: false}): LineLog {
  const log = new LineLog({trackDeps});
  textList.forEach(text => log.recordText(text));
  return log;
}
