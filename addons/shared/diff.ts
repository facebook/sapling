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
type LineIdx = number;

/**
 * Calculate the line differences. For performance, this function only
 * returns the line indexes for different chunks. The line contents
 * are not returned.
 *
 * @param aLines lines on the "a" side.
 * @param bLines lines on the "b" side.
 * @returns A list of `(a1, a2, b1, b2)` tuples for the line ranges that
 * are different between "a" and "b".
 */
export function diffLines(
  aLines: string[],
  bLines: string[],
): [LineIdx, LineIdx, LineIdx, LineIdx][] {
  // Avoid O(string length) comparison.
  const [aList, bList] = stringsToInts([aLines, bLines]);

  // Skip common prefix and suffix.
  let aLen = aList.length;
  let bLen = bList.length;
  const minLen = Math.min(aLen, bLen);
  let commonPrefixLen = 0;
  while (commonPrefixLen < minLen && aList[commonPrefixLen] === bList[commonPrefixLen]) {
    commonPrefixLen += 1;
  }
  while (aLen > commonPrefixLen && bLen > commonPrefixLen && aList[aLen - 1] === bList[bLen - 1]) {
    aLen -= 1;
    bLen -= 1;
  }
  aLen -= commonPrefixLen;
  bLen -= commonPrefixLen;

  // Run the diff algorithm.
  const blocks: [LineIdx, LineIdx, LineIdx, LineIdx][] = [];
  let a1 = 0;
  let b1 = 0;

  function isCommon(aIndex: number, bIndex: number) {
    return aList[aIndex + commonPrefixLen] === bList[bIndex + commonPrefixLen];
  }

  function foundSequence(n: LineIdx, a2: LineIdx, b2: LineIdx) {
    if (a1 !== a2 || b1 !== b2) {
      blocks.push([
        a1 + commonPrefixLen,
        a2 + commonPrefixLen,
        b1 + commonPrefixLen,
        b2 + commonPrefixLen,
      ]);
    }
    a1 = a2 + n;
    b1 = b2 + n;
  }

  diffSequences(aLen, bLen, isCommon, foundSequence);
  foundSequence(0, aLen, bLen);

  return blocks;
}

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
