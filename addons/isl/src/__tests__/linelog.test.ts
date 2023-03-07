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
});

function logFromTextList(textList: string[]): LineLog {
  const log = new LineLog();
  textList.forEach(text => log.recordText(text));
  return log;
}
