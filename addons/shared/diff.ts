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

// Read D43857949 about the choice of the diff library.
import diffSequences from 'diff-sequences';

/** Index of a line. Starts from 0. */
export type LineIdx = number;

/**
 * Calculate the line differences. For performance, this function only
 * returns the line indexes for different chunks. The line contents
 * are not returned.
 *
 * @param aLines lines on the "a" side.
 * @param bLines lines on the "b" side.
 * @param readable whether to use `readableDiffBlocks`.
 * @returns A list of `(a1, a2, b1, b2)` tuples for the line ranges that
 * are different between "a" and "b".
 */
export function diffLines(
  aLines: string[],
  bLines: string[],
  readable = true,
): [LineIdx, LineIdx, LineIdx, LineIdx][] {
  return (readable ? readableDiffBlocks : diffBlocks)(aLines, bLines)
    .filter(([sign, _range]) => sign === '!')
    .map(([_sign, range]) => range);
}

/**
 * Calculate the line differences. For performance, this function returns
 * line ranges not line contents.
 *
 * Similar to Mercurial's `mdiff.allblocks`.
 *
 * @param aLines lines on the "a" side.
 * @param bLines lines on the "b" side.
 * @returns A list of `[sign, [a1, a2, b1, b2]]` tuples for the line ranges.
 * If `sign` is `'='`, the a1 to a2 range on the a side, and b1 to b2 range
 * on the b side are the same on both sides. Otherwise, `sign` is `'!'`
 * meaning the ranges are different.
 */
export function diffBlocks(aLines: string[], bLines: string[]): Array<Block> {
  // Avoid O(string length) comparison.
  const [aList, bList] = stringsToInts([aLines, bLines]);
  return diffIntBlocks(aList, bList);
}

/**
 * Similar to `diffBlocks`, but attempt to produce more human-readable diff.
 *
 * Most of the time, a diff is less human-readable because "insignificant"
 * lines like "{" or "}" are matched. Practically, matching the actual code
 * (significant) lines should have high priority for "readability".
 *
 * To prioritize significant lines, run the main diff algorithm on the
 * significant lines first, force match the equal lines. Those equal lines
 * will split the insignificant lines into smaller regions. Run the diff
 * algorithm on those regions.
 *
 * Because insignificant lines are skipped, this function might not always
 * produce the theoretical minimal diff like `diffBlocks`.
 */
export function readableDiffBlocks(aLines: string[], bLines: string[]): Array<Block> {
  const aIsSignificant = aLines.map(isSignificantLine);
  const bIsSignificant = bLines.map(isSignificantLine);
  const insignificantCount =
    aIsSignificant.reduce((a, b) => a + (b ? 0 : 1), 0) +
    bIsSignificant.reduce((a, b) => a + (b ? 0 : 1), 0);
  if (insignificantCount > (aLines.length + bLines.length + 3) / 2) {
    // Too many insignificant lines. The algorithm might be a bad fit.
    // Use regular diff instead.
    return diffBlocks(aLines, bLines);
  }

  // Assign integer ids to lines.
  const [aFull, bFull] = stringsToInts([aLines, bLines]);

  // Significant lines.
  const aSignificant = aFull.filter((_l, i) => aIsSignificant[i]);
  const bSignificant = bFull.filter((_l, i) => bIsSignificant[i]);

  // Index offset. aSignificant[i] == aFull[aSigToFull[i]].
  const aSigToFull = calculateSigToFull(aIsSignificant);
  const bSigToFull = calculateSigToFull(bIsSignificant);

  let aLast = 0;
  let bLast = 0;
  const result: Array<Block> = [];
  const push = (blocks: ReadonlyArray<Block>, aOffset = 0, bOffset = 0) => {
    blocks.forEach(([sign, [origA1, origA2, origB1, origB2]]) => {
      const a1 = origA1 + aOffset;
      const a2 = origA2 + aOffset;
      const b1 = origB1 + bOffset;
      const b2 = origB2 + bOffset;
      const last = result.at(-1);
      if (last?.[0] === sign && last[1][1] === a1 && last[1][3] === b1) {
        last[1][1] = a2;
        last[1][3] = b2;
      } else {
        result.push([sign, [a1, a2, b1, b2]]);
      }
    });
  };

  diffIntBlocks(aSignificant, bSignificant).forEach(
    ([sign, [aSigStart, aSigEnd, bSigStart, _bSigEnd]]) => {
      if (sign === '=') {
        for (let a1Sig = aSigStart; a1Sig < aSigEnd; a1Sig++) {
          const b1Sig = bSigStart + (a1Sig - aSigStart);
          const [a1, b1] = [aSigToFull[a1Sig], bSigToFull[b1Sig]];
          const [a2, b2] = [a1 + 1, b1 + 1];
          // Force match the two sides. Run regular diff on the insignificant region.
          //   .. aLast   | aLast .. a1   | a1 .. a2      | a2 ...
          //   .. aLast   | bLast .. b1   | b1 .. b2      | b2 ...
          //   ----------------------------------------------------------------
          //   already in | insignificant | significant = | rest
          //   result     | to diff       | force matched | to figure out later
          push(diffIntBlocks(aFull.slice(aLast, a1), bFull.slice(bLast, b1)), aLast, bLast);
          push([['=', [a1, a2, b1, b2]]]);
          [aLast, bLast] = [a2, b2];
        }
      }
    },
  );
  if (aLast < aFull.length || bLast < bFull.length) {
    push(diffIntBlocks(aFull.slice(aLast), bFull.slice(bLast)), aLast, bLast);
  }
  return result;
}

/**
 * Given a boolean array a[i], calculate b[i] so b[i] is the i-th `true` index in a[i],
 * aka. a.slice(0, b[i]) has a total of `i` `true` values.
 */
function calculateSigToFull(isSignificant: ReadonlyArray<boolean>): ReadonlyArray<number> {
  const result: number[] = [];
  for (let i = 0; i < isSignificant.length; i++) {
    if (isSignificant[i]) {
      result.push(i);
    }
  }
  result.push(isSignificant.length);
  return result;
}

/**
 * Test if a line is "significant". Used by `readableDiffBlocks`.
 * Blank lines, "{", "}", "/**" are insignificant.
 */
function isSignificantLine(line: string): boolean {
  return line.match(/^[\s{}/*]*$/) == null;
}

/**
 * Similar to `diffBlocks`, but takes integer array instead.
 */
function diffIntBlocks(aList: ReadonlyArray<number>, bList: ReadonlyArray<number>): Array<Block> {
  // Skip common prefix and suffix.
  let aLen = aList.length;
  let bLen = bList.length;
  const minLen = Math.min(aLen, bLen);
  let commonPrefixLen = 0;
  let commonSuffixLen = 0;
  while (commonPrefixLen < minLen && aList[commonPrefixLen] === bList[commonPrefixLen]) {
    commonPrefixLen += 1;
  }
  while (aLen > commonPrefixLen && bLen > commonPrefixLen && aList[aLen - 1] === bList[bLen - 1]) {
    aLen -= 1;
    bLen -= 1;
    commonSuffixLen += 1;
  }
  aLen -= commonPrefixLen;
  bLen -= commonPrefixLen;

  const blocks: Array<Block> = [];
  if (commonPrefixLen > 0) {
    blocks.push(['=', [0, commonPrefixLen, 0, commonPrefixLen]]);
  }

  // Run the diff algorithm.
  let a1 = 0;
  let b1 = 0;

  function isCommon(aIndex: number, bIndex: number) {
    return aList[aIndex + commonPrefixLen] === bList[bIndex + commonPrefixLen];
  }

  function foundSequence(n: LineIdx, a2: LineIdx, b2: LineIdx) {
    if (a1 !== a2 || b1 !== b2) {
      blocks.push([
        '!',
        [a1 + commonPrefixLen, a2 + commonPrefixLen, b1 + commonPrefixLen, b2 + commonPrefixLen],
      ]);
    }
    if (n > 0) {
      blocks.push([
        '=',
        [
          a2 + commonPrefixLen,
          a2 + n + commonPrefixLen,
          b2 + commonPrefixLen,
          b2 + n + commonPrefixLen,
        ],
      ]);
    }
    a1 = a2 + n;
    b1 = b2 + n;
  }

  diffSequences(aLen, bLen, isCommon, foundSequence);
  foundSequence(commonSuffixLen, aLen, bLen);

  return blocks;
}

/**
 * Post process `blocks` from `diffBlocks` to collapse unchanged lines.
 * `contextLineCount` lines before or after a `!` (changed) block are
 * not collapsed.
 *
 * If `isALineExpanded(aLine, bLine)` returns `true`, then the _block_
 * is expanded.
 *
 * Split `=` blocks into `=` and `~` blocks. The `~` blocks are expected
 * to be collapsed.
 */
export function collapseContextBlocks(
  blocks: Array<Block>,
  isLineExpanded: (aLine: LineIdx, bLine: LineIdx) => boolean,
  contextLineCount = 3,
): Array<ContextBlock> {
  const collapsedBlocks: Array<ContextBlock> = [];
  blocks.forEach((block, i) => {
    const [sign, [a1, a2, b1, b2]] = block;
    if (sign === '!') {
      collapsedBlocks.push(block);
    } else if (sign === '=') {
      // a1 ... a1 + topContext ... a2 - bottomContext ... a2
      //                        ^^^ collapse this range (c1 to c2)
      // The topContext and bottomContext can be 0 lines if they are not adjacent
      // to a diff block.
      const topContext = i == 0 || blocks[i - 1][0] !== '!' ? 0 : contextLineCount;
      const bottomContext =
        i + 1 == blocks.length || blocks[i + 1][0] !== '!' ? 0 : contextLineCount;
      const c1 = Math.min(a1 + topContext, a2);
      const c2 = Math.max(c1, a2 - bottomContext);
      const aToB = b1 - a1;
      if (c1 >= c2 || isLineExpanded(c1, c1 + aToB) || isLineExpanded(c2 - 1, c2 - 1 + aToB)) {
        // Nothing to collapse.
        collapsedBlocks.push(block);
      } else {
        // Split. Collapse c1 .. c2 range.
        if (c1 > a1) {
          collapsedBlocks.push(['=', [a1, c1, b1, c1 + aToB]]);
        }
        collapsedBlocks.push(['~', [c1, c2, c1 + aToB, c2 + aToB]]);
        if (c2 < a2) {
          collapsedBlocks.push(['=', [c2, a2, c2 + aToB, b2]]);
        }
      }
    }
  });
  return collapsedBlocks;
}

/**
 * Merge diffBlocks(a, b) and diffBlocks(c, b).
 * Any difference (between a and b, or c and b) generates a `!` block.
 * The (a1, a2) line numbers in the output blocks are changed to (b1, b2).
 * Preserve empty (a1 == a2, b1 == b2) '!' blocks for context line calculation.
 */
export function mergeBlocks(abBlocks: Array<Block>, cbBlocks: Array<Block>): Array<Block> {
  let i = 0; // Index of abBlocks.
  let j = 0; // Index of cbBlocks.
  let start = 0; // "Current" line index of b.
  const result: Array<Block> = [];

  const push = (sign: Sign, end: number) => {
    const last = result.at(-1);
    if (last?.[0] === sign) {
      last[1][1] = end;
      last[1][3] = end;
    } else {
      result.push([sign, [start, end, start, end]]);
    }
    start = end;
  };

  while (i < abBlocks.length && j < cbBlocks.length) {
    const [sign1, [, , b11, b12]] = abBlocks[i];
    if (b11 === b12 && b12 === start && sign1 === '!') {
      push(sign1, start);
    }
    if (b12 <= start) {
      ++i;
      continue;
    }
    const [sign2, [, , b21, b22]] = cbBlocks[j];
    if (b21 === b22 && b21 === start && sign2 === '!') {
      push(sign2, start);
    }
    if (b22 <= start) {
      ++j;
      continue;
    }

    // Minimal "end" so there cannot be 2 different signs in the start-end range
    // on either side. Note 2 sides might have different signs.
    const end = Math.min(...[b11, b12, b21, b22].filter(i => i > start));

    // Figure out the sign of the start-end range.
    let sign: Sign = '=';
    if (
      (start >= b11 && end <= b12 && sign1 === '!') ||
      (start >= b21 && end <= b22 && sign2 === '!')
    ) {
      sign = '!';
    }
    push(sign, end);
  }

  return result;
}

/** Indicates whether a block is same or different on both sides. */
export type Sign = '=' | '!';

/** Return type of `diffBlocks`. */
export type Block = [Sign, [LineIdx, LineIdx, LineIdx, LineIdx]];

/** Return type of `collapseContextLines`. */
export type ContextBlock = [Sign | '~', [LineIdx, LineIdx, LineIdx, LineIdx]];

/**
 * Split lines by `\n`. Preserve the end of lines.
 */
export function splitLines(s: string): string[] {
  let pos = 0;
  let nextPos = 0;
  const result = [];
  while (pos < s.length) {
    nextPos = s.indexOf('\n', pos);
    if (nextPos === -1) {
      nextPos = s.length - 1;
    }
    result.push(s.slice(pos, nextPos + 1));
    pos = nextPos + 1;
  }
  return result;
}

/**
 * Make strings with the same content use the same integer
 * for fast comparison.
 */
function stringsToInts(linesArray: string[][]): number[][] {
  // This is similar to diff-match-patch's diff_linesToChars_ but is not
  // limited to 65536 unique lines.
  const lineMap = new Map<string, number>();
  return linesArray.map(lines =>
    lines.map(line => {
      const existingId = lineMap.get(line);
      if (existingId != null) {
        return existingId;
      } else {
        const id = lineMap.size;
        lineMap.set(line, id);
        return id;
      }
    }),
  );
}
