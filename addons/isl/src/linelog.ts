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

import type {LRUWithStats} from 'shared/LRU';

import {assert} from './utils';
// Read D43857949 about the choice of the diff library.
import diffSequences from 'diff-sequences';
import {List, Record} from 'immutable';
import {cached, LRU} from 'shared/LRU';
import {unwrap} from 'shared/utils';

/** Operation code. */
enum Op {
  /** Unconditional jump. */
  J = 0,
  /** Jump if the current rev >= operand. */
  JGE = 1,
  /** Jump if the current rev < operand. */
  JL = 2,
  /** Append a line. */
  LINE = 3,
  /** End execution. */
  END = 4,
}

/** J instruction. */
const J = Record(
  {
    /** Opcode: J */
    op: Op.J,
    /** Program counter (offset to jump). */
    pc: 0,
  },
  'J',
);

/** JGE instruction. */
const JGE = Record(
  {
    /** Opcode: JGE */
    op: Op.JGE,
    /** `rev` to test. */
    rev: 0,
    /** Program counter (offset to jump). */
    pc: 0,
  },
  'JGE',
);

/** JL instruction. */
const JL = Record(
  {
    /** Opcode: JL */
    op: Op.JL,
    /** `rev` to test. */
    rev: 0,
    /** Program counter (offset to jump). */
    pc: 0,
  },
  'JL',
);

/** LINE instruction. */
const LINE = Record(
  {
    /** Opcode: LINE */
    op: Op.LINE,
    /** `rev` to test. */
    rev: 0,
    /** Line content. Includes EOL. */
    data: '',
  },
  'LINE',
);

/** END instruction. */
const END = Record(
  {
    /** Opcode: END */
    op: Op.END,
  },
  'END',
);

/** Program counter (offset to instructions). */
type Pc = number;

/** Revision number. Usually starts from 1. Larger number means newer versions. */
type Rev = number;

/** Index of a line. Starts from 0. */
type LineIdx = number;

/** Instruction. */
type Inst = Readonly<
  | {op: Op.J; pc: Pc}
  | {op: Op.END}
  | ((
      | {op: Op.JGE; rev: Rev; pc: Pc}
      | {op: Op.JL; rev: Rev; pc: Pc}
      | {op: Op.LINE; rev: Rev; data: string}
    ) &
      Record<{rev: Rev}>)
>;

/** Information about a line. Internal (`lines`) result of `LineLog.checkOut`. */
interface LineInfo {
  /** Line content. Includes EOL. */
  data: string;
  /** Added by the given rev. */
  rev: Rev;
  /** Produced by the instruction at the given offset. */
  pc: Pc;
  /**
   * Whether the line is deleted.
   * This is always `false` if `checkOut(rev, None)`.
   * It might be `true` when checking out a range of revisions
   * (aka. `start` passed to `checkOut` is not `null`).
   */
  deleted: boolean;
}

/** A "flatten" line. Result of `LineLog.flatten()`. */
interface FlattenLine {
  /** The line is present in the given revisions. */
  revs: Set<Rev>;
  /** Content of the line, including `\n`. */
  data: string;
}

/** List of instructions. */
type Code = List<Inst>;

// Export for testing purpose.
export const executeCache: LRUWithStats = new LRU(100);

const LineLogRecord = Record({
  /** Core state: instructions. The array index type is `Pc`. */
  code: List([END()]) as Code,
  /** Maximum rev tracked. */
  maxRev: 0 as Rev,
});

/**
 * `LineLog` is a data structure that tracks linear changes to a single text
 * file. Conceptually similar to a list of texts like `string[]`, with extra
 * features suitable for stack editing:
 * - Calculate the "blame" of the text of a given version efficiently.
 * - Edit lines or trunks in a past version, and affect future versions.
 * - List all lines that ever existed with each line annotated, like
 *   a unified diff, but for all versions, not just 2 versions.
 *
 * Internally, `LineLog` is a byte-code interpreter that runs a program to
 * emit lines. Changes are done by patching in new byte-codes. There are
 * no traditional text patch involved. No operations would cause merge
 * conflicts. See https://sapling-scm.com/docs/internals/linelog for more
 * details.
 *
 * This implementation of `LineLog` uses immutable patterns.
 * Write operations return new `LineLog`s.
 */
class LineLog extends LineLogRecord {
  /**
   * Edit chunk. Replace line `a1` (inclusive) to `a2` (exclusive) in rev
   * `aRev` with `bLines`. `bLines` are considered introduced by `bRev`.
   * If `bLines` is empty, the edit is a deletion. If `a1` equals to `a2`,
   * the edit is an insertion. Otherwise, the edit is a modification.
   *
   * While this function does not cause conflicts or error out, not all
   * editings make practical sense. The callsite might want to do some
   * extra checks to ensure the edit is meaningful.
   *
   * `aLinesCache` is optional. If provided, then `editChunk` will skip a
   * `checkOutLines` call and modify `aLinesCache` *in place* to reflect
   * the edit. It is used by `recordText`.
   */
  editChunk(
    aRev: Rev,
    a1: LineIdx,
    a2: LineIdx,
    bRev: Rev,
    bLines: string[],
    aLinesCache?: LineInfo[],
  ): LineLog {
    const aLinesMutable = aLinesCache != null;
    const aLines = aLinesMutable ? aLinesCache : this.checkOutLines(aRev);
    const start = this.code.size;

    assert(a1 <= a2, 'illegal chunk (a1 < a2)');
    assert(a2 <= aLines.length, 'out of bound a2 (wrong aRev?)');

    const newCode = this.code.withMutations(origCode => {
      let code = origCode;
      const a1Pc = aLines[a1].pc;
      if (bLines.length > 0) {
        const b2Pc = start + bLines.length + 1;
        code.push(JL({rev: bRev, pc: b2Pc}) as Inst);
        bLines.forEach(line => {
          code = code.push(LINE({rev: bRev, data: line}) as Inst);
        });
        assert(b2Pc === code.size, 'bug: wrong pc');
      }
      if (a1 < a2) {
        const a2Pc = aLines[a2 - 1].pc + 1;
        code = code.push(JGE({rev: bRev, pc: a2Pc}) as Inst);
      }
      if (aLinesMutable) {
        aLinesCache[a1] = {...aLines[a1], pc: code.size};
      }
      code = code.push(unwrap(code.get(a1Pc)));
      switch (unwrap(code.get(a1Pc)).op) {
        case Op.J:
        case Op.END:
          break;
        default:
          code = code.push(J({pc: a1Pc + 1}) as Inst);
      }
      code = code.set(a1Pc, J({pc: start}) as Inst);
      return code;
    });

    if (aLinesMutable) {
      const newLines = bLines.map((s, i) => {
        return {data: s, rev: bRev, pc: start + 1 + i, deleted: false};
      });
      aLinesCache.splice(a1, a2 - a1, ...newLines);
    }

    const newMaxRev = Math.max(bRev, this.maxRev);
    return new LineLog({code: newCode, maxRev: newMaxRev});
  }

  /**
   * Rewrite `rev` to `mapping[rev] ?? rev`.
   * This can be useful for reordering, folding, or insertion.
   *
   * Note: There are no checks about whether the reordering is
   * meaningful or not. The callsite is responsible to perform
   * a dependency check and avoid troublesome reorders like
   * moving a change to before its dependency.
   */
  remapRevs(revMap: Map<Rev, Rev>): LineLog {
    let newMaxRev = 0;
    const newCode = this.code
      .map(c => {
        if (c.op === Op.JGE || c.op === Op.JL || c.op === Op.LINE) {
          const newRev = revMap.get(c.rev) ?? c.rev;
          if (newRev > newMaxRev) {
            newMaxRev = newRev;
          }
          return c.set('rev', newRev);
        }
        return c;
      })
      .toList();
    return new LineLog({code: newCode, maxRev: newMaxRev});
  }

  /**
   * Calculate the dependencies of revisions.
   * For example, `{5: [3, 1]}` means rev 5 depends on rev 3 and rev 1.
   */
  calculateDepMap(): Map<Rev, Set<Rev>> {
    // To calculate dependencies. We look at differences between
    // adjacent revs.
    const depMap = new Map<Rev, Set<Rev>>();
    let leftSide = this.execute(0);
    if (leftSide.length > 1) {
      // rev 0 is non-empty.
      depMap.set(0, new Set());
    }
    for (let rev = 1; rev <= this.maxRev; rev += 1) {
      const rightSide = this.execute(rev);
      diffLines(
        leftSide.map(l => l.data),
        rightSide.map(l => l.data),
      ).forEach(([a1, a2, _b1, _b2]) => {
        let depRevs = depMap.get(rev);
        if (depRevs == null) {
          const newSet = new Set<Rev>();
          depMap.set(rev, newSet);
          depRevs = newSet;
        }
        // Blame surrounding and changed/deleted lines on the left side.
        for (let ai = Math.max(a1 - 1, 0); ai < Math.min(a2 + 1, leftSide.length); ai += 1) {
          const depRev = leftSide[ai].rev;
          if (depRev >= 0 && depRev < rev) {
            depRevs.add(depRev);
          }
        }
      });
      leftSide = rightSide;
    }
    return depMap;
  }

  /**
   * Interpret the bytecodes with the given revision range.
   * Used by `checkOut`.
   */
  @cached({cache: executeCache, cacheSize: 1000})
  execute(
    startRev: Rev,
    endRev: Rev = startRev,
    present?: {[pc: number]: boolean},
  ): Readonly<LineInfo[]> {
    const rev = endRev;
    const lines: LineInfo[] = [];
    let pc = 0;
    let patience = this.code.size * 2;
    const deleted = present == null ? () => false : (pc: Pc) => !present[pc];
    while (patience > 0) {
      const code = unwrap(this.code.get(pc));
      switch (code.op) {
        case Op.END:
          lines.push({data: '', rev: 0, pc, deleted: deleted(pc)});
          patience = -1;
          break;
        case Op.LINE:
          lines.push({data: code.data, rev: code.rev, pc, deleted: deleted(pc)});
          pc += 1;
          break;
        case Op.J:
          pc = code.pc;
          break;
        case Op.JGE:
          if (startRev >= code.rev) {
            pc = code.pc;
          } else {
            pc += 1;
          }
          break;
        case Op.JL:
          if (rev < code.rev) {
            pc = code.pc;
          } else {
            pc += 1;
          }
          break;
        default:
          assert(false, 'bug: unknown code');
      }
      patience -= 1;
    }
    if (patience === 0) {
      assert(false, 'bug: code does not end in time');
    }
    return lines;
  }

  /**
   * Flatten lines. Each returned line is associated with a set
   * of `Rev`s, meaning that line is present in those `Rev`s.
   *
   * The returned lines can be useful to figure out file contents
   * after reordering, folding commits. It can also provide a view
   * similar to `absorb -e FILE` to edit all versions of a file in
   * a single view.
   *
   * Note: This is currently implemented naively as roughly
   * `O(lines * revs)`. Avoid calling frequently for large
   * stacks.
   */
  public flatten(): FlattenLine[] {
    const allLines = this.checkOutLines(this.maxRev, 0);
    // Drop the last (empty) line.
    const len = Math.max(allLines.length - 1, 0);
    const lineInfos = allLines.slice(0, len);
    const linePcs = lineInfos.map(info => info.pc);
    const result: FlattenLine[] = lineInfos.map(info => ({
      revs: new Set<Rev>(),
      data: info.data,
    }));
    for (let rev = 0; rev <= this.maxRev; rev += 1) {
      const lines = this.checkOutLines(rev);
      // Pc is used as the "unique" line identifier to detect what
      // subset of "all lines" exist in the current "rev".
      const pcSet: Set<Pc> = new Set(lines.map(info => info.pc));
      for (let i = 0; i < linePcs.length; i += 1) {
        if (pcSet.has(linePcs[i])) {
          result[i].revs.add(rev);
        }
      }
    }
    return result;
  }

  /**
   * Checkout the lines of the given revision `rev`.
   *
   * If `start` is not `null`, checkout a revision range. For example,
   * if `start` is 0, and `rev` is `this.maxRev`, `this.lines` will
   * include all lines ever existed in all revisions.
   *
   * @returns Content of the specified revision.
   */
  public checkOutLines(rev: Rev, start: Rev | null = null): Readonly<LineInfo[]> {
    // eslint-disable-next-line no-param-reassign
    rev = Math.min(rev, this.maxRev);
    let lines = this.execute(rev);
    if (start !== null) {
      // Checkout a range, including deleted revs.
      const present: {[key: number]: boolean} = {};
      lines.forEach(l => {
        present[l.pc] = true;
      });

      // Go through all lines again. But do not skip chunks.
      lines = this.execute(start, rev, present);
    }
    return lines;
  }

  /** Checkout the content of the given rev. */
  public checkOut(rev: Rev): string {
    const lines = this.checkOutLines(rev);
    const content = lines.map(l => l.data).join('');
    return content;
  }

  /**
   * Edit LineLog to match the content of `text`.
   * This might affect `rev`s that are >= `rev` in the stack.
   * Previous revisions won't be affected.
   *
   * @param text Content to match.
   * @param rev Revision to to edit (in-place). If not set, append a new revision.
   * @returns A new `LineLog` with the change.
   */
  public recordText(text: string, rev: Rev | null = null): LineLog {
    // rev to edit from, and rev to match 'text'.
    const [aRev, bRev] = rev != null ? [rev, rev] : [this.maxRev, this.maxRev + 1];
    const b = text;

    const aLineInfos = [...this.checkOutLines(aRev)];
    const bLines = splitLines(b);
    const aLines = aLineInfos.map(l => l.data);
    aLines.pop(); // Drop the last END empty line.
    const blocks = diffLines(aLines, bLines);
    let log: LineLog = new LineLog(this);

    blocks.reverse().forEach(([a1, a2, b1, b2]) => {
      log = log.editChunk(aRev, a1, a2, bRev, bLines.slice(b1, b2), aLineInfos);
    });

    // This is needed in case editChunk is not called (no difference).
    const newMaxRev = Math.max(bRev, log.maxRev);

    // Populate cache for checking out bRev.
    const newLog = new LineLog({code: log.code, maxRev: newMaxRev});
    executeCache.set(List([newLog, bRev]), aLineInfos);

    return newLog;
  }
}

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
function diffLines(aLines: string[], bLines: string[]): [LineIdx, LineIdx, LineIdx, LineIdx][] {
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
function splitLines(s: string): string[] {
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
 * for fast comparasion.
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

export {LineLog};
export type {FlattenLine, Rev, LineIdx, LineInfo};
