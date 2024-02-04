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

import type {LineIdx, Rev} from '../linelog';

import {LineLog, executeCache} from '../linelog';
import {describe, it, expect} from '@jest/globals';
import * as Immutable from 'immutable';
import {splitLines} from 'shared/diff';

describe('LineLog', () => {
  it('can be empty', () => {
    const log = new LineLog();
    expect(log.maxRev).toBe(0);
    expect(log.checkOut(0)).toBe('');
  });

  it('supports a single edit', () => {
    let log = new LineLog();
    log = log.recordText('c\nd\ne');
    expect(log.maxRev).toBe(1);
    expect(log.checkOut(1)).toBe('c\nd\ne');

    expect(log.checkOutLines(1)).toMatchObject([
      {data: 'c\n', rev: 1},
      {data: 'd\n', rev: 1},
      {data: 'e', rev: 1},
      {data: '', rev: 0},
    ]);
  });

  it('supports modifying rev 0', () => {
    let log = new LineLog();
    log = log.recordText('c\n', 0);
    expect(log.maxRev).toBe(0);
    expect(log.checkOut(0)).toBe('c\n');
    expect(log.checkOutLines(0)[0]).toMatchObject({rev: 0});
    log = log.recordText('c\nd', 1);
    expect(log.checkOutLines(1)[1]).toMatchObject({rev: 1});
    expect(log.checkOut(0)).toBe('c\n');
    expect(log.checkOutLines(0)[0]).toMatchObject({rev: 0});
  });

  it('supports multiple edits', () => {
    let log = new LineLog();
    log = log.recordText('c\nd\ne\n');
    log = log.recordText('d\ne\nf\n');
    expect(log.maxRev).toBe(2);
    expect(log.checkOut(2)).toBe('d\ne\nf\n');
    expect(log.checkOutLines(2)).toMatchObject([
      {data: 'd\n', rev: 1, deleted: false},
      {data: 'e\n', rev: 1, deleted: false},
      {data: 'f\n', rev: 2, deleted: false},
      {data: '', rev: 0, deleted: false},
    ]);
  });

  it('supports checkout', () => {
    let log = new LineLog();
    log = log.recordText('c\nd\ne\n');
    log = log.recordText('d\ne\nf\n');
    expect(log.checkOut(1)).toBe('c\nd\ne\n');
    expect(log.checkOutLines(1)[0].deleted).toBe(false);
    expect(log.checkOut(0)).toBe('');
    expect(log.checkOutLines(0)).toMatchObject([{data: ''}]);
    expect(log.checkOut(2)).toBe('d\ne\nf\n');
    expect(log.checkOutLines(2)[2].deleted).toBe(false);
  });

  it('supports checkout range', () => {
    const log = new LineLog()
      .recordText('c\nd\ne\n') // rev 1
      .recordText('d\ne\nf\n') // rev 2
      .recordText('e\ng\nf\n'); // rev 3

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
    let log = new LineLog();
    log = log.recordText('a\n');
    expect(log.maxRev).toBe(1);
    log = log.recordText('a\n');
    expect(log.maxRev).toBe(2);
    log = log.recordText('a\n');
    expect(log.maxRev).toBe(3);
  });

  it('avoids checkout/execute calls for common edits', () => {
    const log = new LineLog().recordText('a\nb\nc\nd\ne\n', 1);

    // Modifies 3 chunks. This does not introduce new cache
    // miss, because:
    // - checkout (calls execute) used by recordText can
    //   reuse cache populated by the previous recordText.
    //   This contributes a cache hit.
    // - checkout used by editChunk is skipped, because
    //   recordText passes in `aLinesCache`. This does not
    //   change cache miss or hit.
    const stats = (executeCache.stats = {miss: 0, hit: 0});
    log.recordText('A\nb\nC\nd\nE\n', 3);
    expect(stats).toMatchObject({miss: 0, hit: 1});
  });

  it('works with immutable.is', () => {
    const log1 = new LineLog().recordText('a').recordText('b');
    const log2 = new LineLog({code: log1.code, maxRev: log1.maxRev});
    const log3 = new LineLog().recordText('a').recordText('b');

    expect(Object.is(log1, log2)).toBeFalsy();
    expect(Immutable.is(log1, log2)).toBeTruthy();
    expect(Immutable.is(log1, log3)).toBeTruthy();
  });

  it('provides human readable instructions', () => {
    const log = logFromTextList(['a\n', 'b\n']);
    // The instructions are internal details. For example, an
    // optimization pass might remove unconditional jumps.
    // Shall the output change, just update the test here.
    expect(log.code.describeHumanReadableInstructions()).toEqual([
      '0: J 1',
      '1: JL 1 3',
      '2: J 4',
      '3: END',
      '4: JL 2 6',
      '5: LINE 2 "b"',
      '6: JGE 2 3',
      '7: LINE 1 "a"',
      '8: J 3',
    ]);
  });

  describe('provides human readable insertion and deletion stacks', () => {
    const show = (texts: string[]) =>
      logFromTextList(texts).code.describeHumanReadableInsDelStacks();

    it('interleaved insertion and deletion trees', () => {
      // First 3 revs are from https://sapling-scm.com/docs/internals/linelog
      expect(show(['a\nb\nc\n', 'a\nb\n1\n2\nc\n', 'a\n2\nc\n', 'c\n', ''])).toEqual([
        '+----Insert (rev 1)         ',
        '|    Delete (rev 4)    ----+',
        '|    Line:  a              |',
        '|    Delete (rev 3)    ---+|',
        '|    Line:  b             ||',
        '|+---Insert (rev 2)       ||',
        '||   Line:  1             ||',
        '||                     ---+|',
        '||   Line:  2              |',
        '|+---                      |',
        '|                      ----+',
        '|    Delete (rev 5)    ----+',
        '|    Line:  c              |',
        '|                      ----+',
        '+----                       ',
      ]);
    });

    it('insertions at the beginning and end are not nested', () => {
      expect(show(['b\n', 'a\nb\n', 'a\nb\nc\n'])).toEqual([
        '+---Insert (rev 2)       ',
        '|   Line:  a             ',
        '+---                     ',
        '+---Insert (rev 1)       ',
        '|   Line:  b             ',
        '+---                     ',
        '+---Insert (rev 3)       ',
        '|   Line:  c             ',
        '+---                     ',
      ]);
    });

    it('insertion between old new revs is not nested', () => {
      expect(show(['a\n', 'a\nc\n', 'a\nb\nc\n'])).toEqual([
        '+---Insert (rev 1)       ',
        '|   Line:  a             ',
        '+---                     ',
        '+---Insert (rev 3)       ',
        '|   Line:  b             ',
        '+---                     ',
        '+---Insert (rev 2)       ',
        '|   Line:  c             ',
        '+---                     ',
      ]);
    });

    it('insertion between new old revs is not nested', () => {
      expect(show(['c\n', 'a\nc\n', 'a\nb\nc\n'])).toEqual([
        '+---Insert (rev 2)       ',
        '|   Line:  a             ',
        '+---                     ',
        '+---Insert (rev 3)       ',
        '|   Line:  b             ',
        '+---                     ',
        '+---Insert (rev 1)       ',
        '|   Line:  c             ',
        '+---                     ',
      ]);
    });
  });

  describe('supports editing previous revisions', () => {
    it('edits stack bottom', () => {
      const textList = ['a\n', 'a\nb\n', 'z\na\nb\n'];
      let log = logFromTextList(textList);

      log = log.recordText('1\n2\n', 1); // replace rev 1 from "a" to "1 2"
      expect(log.checkOut(1)).toBe('1\n2\n');
      expect(log.checkOut(2)).toBe('1\n2\nb\n');
      expect(log.checkOut(3)).toBe('z\n1\n2\nb\n');

      log = log.recordText('', 1); // replace rev 1 to ""
      expect(log.checkOut(1)).toBe('');
      expect(log.checkOut(2)).toBe('b\n');
      expect(log.checkOut(3)).toBe('z\nb\n');
    });

    it('edits stack middle', () => {
      const textList = ['c\nd\ne\n', 'b\nc\nd\n', 'a\nb\nc\nz\n'];
      let log = logFromTextList(textList);

      log = log.recordText('b\nd\n', 2); // remove "c" from "b c d" in rev 2
      expect(log.checkOut(1)).toBe('c\nd\ne\n'); // rev 1 is unchanged, despite "c" comes from rev 1
      expect(log.checkOut(2)).toBe('b\nd\n');
      expect(log.checkOut(3)).toBe('a\nb\nz\n'); // "c" in rev 3 is also removed

      log = logFromTextList(textList);
      log = log.recordText('b\nc\ny\ny\n', 2); // change "d" to "y y" from rev 2.
      expect(log.checkOut(3)).toBe('a\nb\nc\nz\n'); // rev 3 is unchanged, since "d" was deleted

      log = logFromTextList(textList);
      log = log.recordText('k\n', 2); // replace rev 2 with "k", this is a tricky case
      expect(log.checkOut(3)).toBe('a\nk\n'); // "a k" is the current implementation, "a k z" might be better
    });
  });

  it('calculates dependencies using linelog instructions', () => {
    const deps = (textList: string[]): (number | number[])[][] => {
      const insertEOL = (text: string): string =>
        text
          .split('')
          .map(c => `${c}\n`)
          .join('');
      const log = logFromTextList(textList.map(insertEOL));
      return flattenDepMap(log.calculateDepMap());
    };

    expect(deps([])).toEqual([]);

    // Insertions.
    expect(deps(['a'])).toEqual([[1, [0]]]);
    expect(deps(['a', 'b'])).toEqual([
      [1, [0]],
      [2, [1]],
    ]);
    expect(deps(['a', 'ab'])).toEqual([
      [1, [0]],
      [2, [0]],
    ]);
    expect(deps(['b', 'ab'])).toEqual([
      [1, [0]],
      [2, [0]],
    ]);
    expect(deps(['ad', 'abd', 'abcd'])).toEqual([
      [1, [0]],
      [2, [1]],
      [3, [1]],
    ]);
    expect(deps(['ad', 'acd', 'abcd'])).toEqual([
      [1, [0]],
      [2, [1]],
      [3, [1]],
    ]);

    // Deletions.
    expect(deps(['abcd', 'abd', 'ad', 'a'])).toEqual([
      [1, [0]],
      [2, [1]],
      [3, [1]],
      [4, [1]],
    ]);
    expect(deps(['abcd', 'acd', 'ad', 'd'])).toEqual([
      [1, [0]],
      [2, [1]],
      [3, [1]],
      [4, [1]],
    ]);

    // Multi-rev insertion, then delete.
    expect(deps(['abc', 'abcdef', '']).at(-1)).toEqual([3, [1, 2]]);
    expect(deps(['abc', 'abcdef', 'af']).at(-1)).toEqual([3, [1, 2]]);
    expect(deps(['abc', 'abcdef', 'cd']).at(-1)).toEqual([3, [1, 2]]);

    // Another test case.
    const textList = ['abc', 'abcd', 'zabcd', 'zad', 'ad', 'adef', 'ade', 'ad1e', 'xyz'];
    expect(deps(textList)).toStrictEqual([
      [1, [0]],
      [2, [0]],
      [3, [0]],
      // deletes "bc" added by rev 1
      [4, [1]],
      // deletes "z" added by rev 3
      [5, [3]],
      // appends after "d" added by rev 2, considered as independent
      [6, [0]],
      // deletes "f" added by rev 6
      [7, [6]],
      // inserts "1" between "d" (rev 2) and "e" (rev 6), still considered as independent
      [8, [0]],
      // replaces all: "a" (rev 1), "d" (rev 2), "1" (rev 8), "e" (rev 6)
      // rev 4 is also a dependent for nested deletions.
      [9, [1, 2, 4, 6, 8]],
    ]);
  });

  it('produces flatten lines', () => {
    const textList = ['a\nb\nc\n', 'b\nc\nd\ne\n', 'a\nc\nd\nf\n'];
    const log = logFromTextList(textList);
    const lines = log.flatten();
    expect(lines.toJS()).toEqual(
      [
        ['a', [1]],
        ['a', [3]],
        ['b', [1, 2]],
        ['c', [1, 2, 3]],
        ['d', [2, 3]],
        ['f', [3]],
        ['e', [2]],
      ].map(([line, revs]) => ({revs, data: `${line}\n`})),
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

  // Ported from test-linelog-edits.py (D3709431)
  // Compare LineLog.editChunk against List<string>.splice edits.
  it('stress tests against random edits', () => {
    const maxDeltaA = 10; // max(a2 - a1)
    const maxDeltaB = 10; // max(b2 - b1)
    const maxB1 = 0xffffff;

    function randInt(min: number, max: number): number {
      return Math.floor(Math.random() * (max - min + 1) + min);
    }

    function randBool(): boolean {
      return Math.random() < 0.5;
    }

    function* generateCases(
      endRev = 1000,
    ): Generator<[Immutable.List<string>, Rev, LineIdx, LineIdx, LineIdx, LineIdx, string[]]> {
      // Maintain `lines` as an alternative to LineLog
      let lines: Immutable.List<string> = Immutable.List();
      for (let rev = 0; rev <= endRev; ++rev) {
        const n = lines.size;
        const a1 = randInt(0, n);
        const a2 = randInt(a1, Math.min(n, a1 + maxDeltaA));
        const b1 = randInt(0, maxB1);
        const b2 = randInt(b1, b1 + maxDeltaB);
        const bLines: string[] = [];
        for (let bIdx = b1; bIdx < b2; bIdx++) {
          const line = randBool() ? '\n' : `${rev}:${bIdx}\n`;
          bLines.push(line);
        }
        lines = lines.splice(a1, a2 - a1, ...bLines);
        yield [lines, rev, a1, a2, b1, b2, bLines];
      }
    }

    const cases = [...generateCases()];
    let log = new LineLog();

    // The use of aLines cache prevents cache miss.
    // It can reduce editChunk time for 100 revs from 240ms to 8ms.
    const aLines = [...log.checkOutLines(0)];
    executeCache.stats = {miss: 0};
    cases.forEach(([_lines, rev, a1, a2, _b1, _b2, bLines]) => {
      log = log.editChunk(log.maxRev, a1, a2, rev, bLines, aLines);
    });
    expect(executeCache.stats).toMatchObject({miss: 0});

    // Check that every rev can be checked out fine.
    cases.forEach(([lines, rev, _a1, _a2, _b1, _b2, _bLines]) => {
      expect(log.checkOut(rev)).toBe(lines.join(''));
    });
  });

  describe('supports remapping revisions', () => {
    it('updates maxRev up', () => {
      const log = logFromTextList(['a', 'b']).remapRevs(new Map([[1, 10]]));
      expect(log.maxRev).toBe(10);
    });

    it('updates maxRev down', () => {
      const log = new LineLog().recordText('a\n', 10).remapRevs(new Map([[10, 5]]));
      expect(log.maxRev).toBe(5);
    });

    it('invalidates previous checkout', () => {
      let log = logFromTextList(['b\n', 'b\nc\n', 'a\nb\nc\n']);
      expect(log.checkOut(2)).toBe('b\nc\n');
      log = log.remapRevs(
        new Map([
          [2, 3],
          [3, 2],
        ]),
      );
      expect(log.checkOut(2)).not.toBe('b\nc\n');
    });

    describe('can reorder changes', () => {
      const defaultSwap: [Rev, Rev] = [2, 3];

      /** Follow the swap. For example, mapSwap(2, [2, 3]) is 3. */
      const mapSwap = (rev: Rev, swap: [Rev, Rev] = defaultSwap) =>
        rev === swap[0] ? swap[1] : rev === swap[1] ? swap[0] : rev;

      /**
       * Swap revs from revs to newRevs. All "line"s are added, by different revs.
       * For example, when lines = ['a\n', 'b\n', 'c\n'], lineAddedOrder = [1, 3, 2],
       * it means 'a' was added by rev 1, 'b' by rev 3, 'c' by rev 2, so the 3 revs
       * are ['a\n', 'a\nc\n', 'a\nb\n'].
       *
       * By default, this test takes 3 revs and swap rev 2 and 3.
       */
      function testReorderInsertions(
        lines: string[],
        lineAddedOrder: Rev[],
        opts?: {
          swap?: [Rev, Rev] /* 2 revs to swap, default: [2, 3] */;
          join?: string /* join character, default: '' */;
          expectedDepMapOverride?: [Rev, Rev[]][] /* override the expected depMap */;
          expectedTextOverride?: string[] /* override the expected texts after swapping */;
        },
      ) {
        expect(lines.length).toBe(lineAddedOrder.length);
        const revs = Array.from({length: lines.length}, (_, i) => i + 1); /* ex. [1, 2, 3] */
        const swap = opts?.swap ?? defaultSwap;
        const optJoin = opts?.join ?? '';
        const getTexts = (revOrder: Rev[], join = optJoin): string[] => {
          const revSet = new Set<Rev>();
          return revOrder.map(rev => {
            revSet.add(rev);
            return lines.filter((_l, i) => revSet.has(lineAddedOrder[i])).join(join);
          });
        };
        const texts = getTexts(revs);
        const log = logFromTextList(texts);

        // Nothing depends on each other.
        expect(flattenDepMap(log.calculateDepMap())).toEqual(
          opts?.expectedDepMapOverride ?? revs.map(r => [r, [0]]),
        );

        // Reorder.
        const reorderedLog = log.remapRevs(new Map([swap, [swap[1], swap[0]]]));

        // Check reorder result.
        // Remove "join" for easier comparison.
        const removeJoin = (text: string) =>
          splitLines(text)
            .filter(t => t !== optJoin)
            .join('');
        const reorderedRevs = revs.map(r => mapSwap(r, swap));
        const expectedReorderedText = opts?.expectedTextOverride ?? getTexts(reorderedRevs, '');
        revs.forEach((rev, i) => {
          expect(removeJoin(reorderedLog.checkOut(rev))).toBe(expectedReorderedText[i]);
        });

        return reorderedLog;
      }

      const threeRevPermutations = [
        [1, 2, 3],
        [1, 3, 2],
        [2, 1, 3],
        [2, 3, 1],
        [3, 1, 2],
        [3, 2, 1],
      ];

      /** Make the test a bit more interesting with multi-line input. */
      const charToFunction = (c: string): string => `function ${c} () {\n  return '${c}';\n}\n`;

      const abcTexts = ['a\n', 'b\n', 'c\n'];
      const functionTexts = ['a', 'b', 'c'].map(charToFunction);

      threeRevPermutations.forEach(revOrder => {
        it(`insert 'a','b','c' in ${revOrder} order`, () => {
          const log = testReorderInsertions(abcTexts, revOrder);
          expect(log.checkOutLines(3)).toMatchObject([
            {data: 'a\n', rev: mapSwap(revOrder[0])},
            {data: 'b\n', rev: mapSwap(revOrder[1])},
            {data: 'c\n', rev: mapSwap(revOrder[2])},
            {data: '', rev: 0},
          ]);
        });

        it(`insert 3 functions in ${revOrder} order`, () => {
          testReorderInsertions(functionTexts, revOrder);
        });

        it(`insert 3 functions with new line join, in ${revOrder} order`, () => {
          testReorderInsertions(abcTexts, revOrder, {join: '\n'});
        });
      });
    });

    it('can merge changes', () => {
      const log = logFromTextList(['b\n', 'b\nc\n', 'a\nb\nc\n']).remapRevs(new Map([[2, 1]]));
      expect(log.checkOut(1)).toBe('b\nc\n');
      expect(log.checkOut(2)).toBe('b\nc\n');
      expect(log.checkOut(3)).toBe('a\nb\nc\n');
    });

    it('can insert changes', () => {
      const log = logFromTextList(['b\n', 'b\nc\n'])
        .remapRevs(new Map([[2, 3]]))
        .recordText('a\nb\n', 2);
      expect(log.checkOut(3)).toBe('a\nb\nc\n');
    });

    it('does not check dependencies or conflicts', () => {
      // rev 2: +b between a and c. rev 2 depends on rev 1.
      const log = logFromTextList(['a\nc\n', 'a\nb\nc\n']).remapRevs(
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
  let log = new LineLog();
  textList.forEach(text => (log = log.recordText(text)));
  return log;
}

function flattenDepMap(depMap: Map<Rev, Set<Rev>>): [Rev, Rev[]][] {
  return [...depMap.entries()].map(([rev, set]) => [rev, [...set].sort()] as [Rev, Rev[]]).sort();
}
