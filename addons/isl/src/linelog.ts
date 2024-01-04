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

import type {RecordOf, ValueObject} from 'immutable';
import type {LRUWithStats} from 'shared/LRU';

import {assert} from './utils';
import {hash, List, Record, Set as ImSet} from 'immutable';
import {cached, LRU} from 'shared/LRU';
import {diffLines, splitLines} from 'shared/diff';
import {SelfUpdate} from 'shared/immutableExt';
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
type J = RecordOf<{
  op: Op.J;
  pc: number;
}>;

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
type JGE = RecordOf<{
  op: Op.JGE;
  rev: Rev;
  pc: number;
}>;

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
type JL = RecordOf<{
  op: Op.JL;
  rev: Rev;
  pc: number;
}>;

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
type LINE = RecordOf<{
  op: Op.LINE;
  rev: Rev;
  data: string;
}>;

/** END instruction. */
const END = Record(
  {
    /** Opcode: END */
    op: Op.END,
  },
  'END',
);
type END = RecordOf<{
  op: Op.END;
}>;

/** Program counter (offset to instructions). */
type Pc = number;

/** Revision number. Usually starts from 1. Larger number means newer versions. */
type Rev = number;

/** Index of a line. Starts from 0. */
type LineIdx = number;

/** Instruction. */
type Inst = J | END | JGE | JL | LINE;

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
type FlattenLineProps = {
  /** The line is present in the given revisions. */
  revs: ImSet<Rev>;
  /** Content of the line, including `\n`. */
  data: string;
};
const FlattenLine = Record<FlattenLineProps>({revs: ImSet(), data: ''});
type FlattenLine = RecordOf<FlattenLineProps>;

/** Used by visitWithInsDelStacks */
type Frame = {rev: Rev; endPc: Pc};

/**
 * List of instructions.
 *
 * This is a wrapper of `List<Inst>` for more efficient `hashCode` and `equals`
 * calculations. The default `hashCode` from `immutable.js` scans the whole
 * `List`. In this implementation we keep 2 internal values: hash and str. The
 * `hash` is used for hashCode, and the `str` is an append-only string that
 * tracks the `editChunk` and other operations to `List<Inst>` for testing
 * equality.
 *
 * You might have noticed that the `str` equality might not match the
 * `List<Inst>` equality. For example, if we remap 1 to 2, then remap 2 to 1,
 * the `List<Inst>` is not changed, but the `str` is changed. It is okay to
 * treat the linelogs as different in this case as we almost always immediately
 * rebuild linelogs after a `remap`. It's important to make sure `recordText`
 * with the same text list gets cache hit.
 */
class Code implements ValueObject {
  constructor(
    private instList: List<Inst> = List([END() as Inst]),
    private __hash: Readonly<number> = 0,
    private __valueOf: Readonly<string> = '',
  ) {}

  getSize(): number {
    return this.instList.size;
  }

  get(pc: Pc): Readonly<Inst> | undefined {
    return this.instList.get(pc);
  }

  valueOf(): string {
    return this.__valueOf;
  }

  equals(other: Code): boolean {
    return this.__valueOf === other.__valueOf;
  }

  hashCode(): number {
    return this.__hash;
  }

  /**
   * Dump instructions in a human readable format. Useful for debugging.
   * Note: This exposes internal details which might change in the future.
   */
  describeHumanReadableInstructions(): string[] {
    return this.instList.map((inst, i) => `${i}: ${describeInst(inst)}`).toArray();
  }

  /**
   * Dump lines with ASCII annotated insertions and deletions stacks.
   */
  describeHumanReadableInsDelStacks(): string[] {
    // 1st Pass: Figure out the max stack depth, line length for padding.
    let maxInsStackDepth = 0;
    let maxDelStackDepth = 0;
    let maxLineLength = 'Insert (rev: 1000)'.length;
    this.visitWithInsDelStacks((insStack, delStack) => {
      return {
        onStackPush: () => {
          maxInsStackDepth = Math.max(maxInsStackDepth, insStack.length + 1);
          maxDelStackDepth = Math.max(maxDelStackDepth, delStack.length + 2);
        },
        onLine: line => {
          maxLineLength = Math.max(maxLineLength, line.data.length + 'Line:  '.length);
        },
      };
    });
    // 2nd Pass: Render the instructions.
    const result: string[] = [];
    this.visitWithInsDelStacks((insStack, delStack) => {
      const pushLine = (data: string, leftAdjust?: number, rightAdjust?: number) => {
        const insDepth = insStack.length - 1 + (leftAdjust ?? 0);
        const delDepth = delStack.length + (rightAdjust ?? 0);
        const insPad = maxInsStackDepth - insDepth;
        const delPad = maxDelStackDepth - delDepth;
        const left =
          '|'.repeat(insDepth) +
          (leftAdjust == null ? ' '.repeat(insPad + 1) : `+${'-'.repeat(insPad)}`);
        const right =
          (rightAdjust == null ? ' '.repeat(delPad + 1) : `${'-'.repeat(delPad)}+`) +
          '|'.repeat(delDepth);
        const middle = data + ' '.repeat(maxLineLength - data.length);
        result.push(left + middle + right);
      };
      return {
        onStackPush: stack => {
          const rev = stack.at(-1)?.rev ?? 0;
          if (stack === insStack) {
            // | | +------ Insert (rev x)  <- this line
            // | | |       Line:  ....     <- following lines
            pushLine(`Insert (rev ${rev})`, -1);
          } else {
            pushLine(`Delete (rev ${rev})`, undefined, -1);
          }
        },
        onStackPop: stack => {
          if (stack === insStack) {
            pushLine('', 0);
          } else {
            pushLine('', undefined, 0);
          }
        },
        onLine: line => {
          pushLine(`Line:  ${line.data.trimEnd()}`);
        },
      };
    });
    return result;
  }

  editChunk(
    aRev: Rev,
    a1: LineIdx,
    a2: LineIdx,
    bRev: Rev,
    bLines: string[],
    [aLines, aLinesMutable]: [LineInfo[], true] | [Readonly<LineInfo[]>, false],
  ): Code {
    const start = this.instList.size;

    assert(a1 <= a2, 'illegal chunk (a1 < a2)');
    assert(a2 <= aLines.length, 'out of bound a2 (wrong aRev?)');

    // See also https://sapling-scm.com/docs/internals/linelog/#editing-linelog
    // # Before             # After
    // # (pc): Instruction  # (pc): Instruction
    //       : ...                : ...
    //   a1Pc: <a1Inst>       a1Pc: J start
    // a1Pc+1: ...          a1Pc+1: ...
    //       : ...                : ...
    //   a2Pc: ...            a2Pc: ...
    //       : ...                : ...
    //    len: N/A           start: JL brev b2Pc      [1]
    //                            : LINE brev b1      [1]
    //                            : LINE brev b1+1    [1]
    //                            : ...               [1]
    //                            : LINE brev b2-1    [1]
    //                        b2Pc: JGE brev a2Pc     [2]
    //                            : <a1Inst> (moved)  [3]
    //                            : J a1Pc+1          [4]
    // [1]: Only present if `bLines` is not empty.
    // [2]: Only present if `a1 < a2`.
    //      There are 2 choices for "a2Pc":
    //      - The a2 line exactly: aLines[a2].pc
    //      - The next instruction of the "a2 -1" line: aLines[a2 - 1].pc + 1
    //      We pick the latter to avoid overly aggressive deletion.
    //      The original C implementation might pick the former when editing
    //      the last rev for performance optimization.
    // [3]: <a1 Inst> could be LINE or END.
    // [4]: As an optimization, this is only present if <a1 Inst> is not END.
    //
    // Optimization [OPT1] to make reorder less restrictive, treat insertion
    // (a1 == a2) at the beginning of another insertion (<a1 Inst> is after a
    // <JL>) specially. Our goal is to avoid nested JLs. Instead of patching
    // the a1Inst after the JL, we patch the JL (jlInst) so we can insert our
    // new JL (for this edit) before the old JL (jlInst, being patched).
    // Note this "JL followed by a1Inst" optimization needs to be applicable
    // multiple times. To do that, we also move the a1Inst to right after the
    // jlInst so the pattern "JL followed by a1Inst" can be recognized by the
    // next editChunk to apply the same optimization.
    //
    // # Before             # After
    // # (pc): Instruction  # (pc): Instruction
    //       : ...                : ...
    //       : <jlInst>     a1Pc-1: J start           [*]
    //   a1Pc: <a1Inst>       a1Pc: NOP (J a1Pc+1)    [*]
    //       : ...                : ...
    //    len: N/A           start: JL brev b2Pc
    //                            : (bLines)
    //                        b2Pc: <jlInst> (moved)  [*]
    //                            : <a1Inst> (moved)
    //                            : J a1Pc            [*]
    const newInstList = this.instList.withMutations(origCode => {
      let code = origCode;
      const a1Pc = aLines[a1].pc;
      // If `jlInst` is set, optimization [OPT1] is in effect.
      let jlInst = a1Pc > 0 && a1 === a2 ? code.get(a1Pc - 1) : undefined;
      if (jlInst?.op !== Op.JL) {
        jlInst = undefined;
      }
      if (bLines.length > 0) {
        // [1]
        const b2Pc = start + bLines.length + 1;
        code = code.push(JL({rev: bRev, pc: b2Pc}) as Inst);
        bLines.forEach(line => {
          code = code.push(LINE({rev: bRev, data: line}) as Inst);
        });
        assert(b2Pc === code.size, 'bug: wrong pc');
      }
      if (a1 < a2) {
        assert(jlInst === undefined, 'OPT1 requires no deletion');
        // [2]
        const a2Pc = aLines[a2 - 1].pc + 1;
        code = code.push(JGE({rev: bRev, pc: a2Pc}) as Inst);
      }
      if (aLinesMutable) {
        aLines[a1] = {...aLines[a1], pc: jlInst == null ? code.size : code.size + 1};
      }
      const a1Inst = unwrap(code.get(a1Pc));
      if (jlInst === undefined) {
        // [3]
        code = code.push(a1Inst);
        if (a1Inst.op /* LINE or END */ !== Op.END) {
          // [4]
          code = code.push(J({pc: a1Pc + 1}) as Inst);
        }
        code = code.set(a1Pc, J({pc: start}) as Inst);
      } else {
        code = code
          .push(jlInst)
          .push(a1Inst)
          .push(J({pc: a1Pc}) as Inst)
          .set(a1Pc - 1, J({pc: start}) as Inst)
          .set(a1Pc, J({pc: a1Pc + 1}) as J);
      }
      return code;
    });

    if (aLinesMutable) {
      const newLines = bLines.map((s, i) => {
        return {data: s, rev: bRev, pc: start + 1 + i, deleted: false};
      });
      aLines.splice(a1, a2 - a1, ...newLines);
    }

    const newValueOf = `E${aRev},${a1},${a2},${bRev},${bLines.join('')}`;
    return this.newCode(newInstList, newValueOf);
  }

  /**
   * Visit (execute) instructions with the insertion and deletion stacks
   * converted from JGE and JL instructions maintained by this function.
   *
   * See the comment in this function about how to turn JGE and JL to
   * the stacks.
   *
   * For stacks like this:
   *
   *    +---- Insertion (rev 1)
   *    |     Line 1
   *    |                    ----+ Deletion (rev 4)
   *    |     Line 2             |
   *    | +-- Insertion (rev 2)  |
   *    | |   Line 3             |
   *    | |                  --+ | Deletion (rev 3)
   *    | |   Line 4           | |
   *    | +--                  | |
   *    |     Line 5           | |
   *    |                    --+ |
   *    |     Line 6             |
   *    |                    ----+
   *    |     Line 7
   *    +----
   *
   * When visiting "Line 3", the callsite will get insertion stack =
   * [rev 1, rev 2] and deletion stack = [rev 4].
   *
   * Internally, this is done by turning conditional jumps (JGE or JL)
   * to stack pushes, pops at the JGE or JL destinations, and follow
   * unconditional jumps (J) as usual. For more details, see the comment
   * inside this function.
   *
   * This function will call `withContext` to provide the `insStack` and
   * `delStack` context, and expect the callsite to provide handlers it
   * is interested in.
   *
   * Typical use-cases include features that need to scan all (ever existed)
   * lines like flatten() and calculateDepMap().
   */
  visitWithInsDelStacks(
    withContext: (
      insStack: Readonly<Frame[]>,
      delStack: Readonly<Frame[]>,
    ) => {
      /** Before stack pop or push */
      onPc?: (pc: number) => void;
      /** After stack pop, before stack push */
      onLine?: (inst: LINE) => void;
      /** After stack pop, before stack push */
      onConditionalJump?: (inst: JGE | JL) => void;
      /** After stack push */
      onStackPush?: (stack: Readonly<Frame[]>) => void;
      /** After stack pop */
      onStackPop?: (stack: Readonly<Frame[]>) => void;
    },
  ) {
    // How does it work? First, insertions and deletions in linelog form
    // tree structures. For example:
    //
    //    +---- Insertion (rev 1)
    //    |     Line 1
    //    |                    ----+ Deletion (rev 4)
    //    |     Line 2             |
    //    | +-- Insertion (rev 2)  |
    //    | |   Line 3             |
    //    | |                  --+ | Deletion (rev 3)
    //    | |   Line 4           | |
    //    | +--                  | |
    //    |     Line 5           | |
    //    |                    --+ |
    //    |     Line 6             |
    //    |                    ----+
    //    |     Line 7
    //    +----
    //
    // Note interleaved insertions do not happen. For example, this does not
    // happen:
    //
    //    +---- Insertion (rev 1)
    //    |     Line 1
    //    | +-- Insertion (rev 2)
    //    | |   Line 2
    //    +-|--
    //      |   Line 3
    //      +--
    //
    // Similarly, interleaved deletions do not happen. However, insertions
    // might interleave with deletions, as shown above.
    //
    // Let's look at how this is done at the instruction level. First, look at
    // the instructions generated by editChunk:
    //
    //      a2Pc: ...
    //            ...
    //     start: JL brev b2Pc
    //            ...
    //      b2Pc: JGE brev a2Pc
    //          : <a1 Inst>
    //       end: J a1Pc+1
    //
    // JL is used for insertion, JGE is used for deletion. We then use them to
    // manipulate the insStack and delStack:
    //
    // insStack:
    //
    //    - On "start: JL brev b2Pc":
    //      Do not follow the JL jump.
    //      Push {rev, b2Pc} to insStack.
    //    - When pc is b2Pc, pop insStack.
    //
    // delStack:
    //
    //    - On "b2Pc: JGE brev a2Pc":
    //      Do not follow the JGE jump.
    //      Push {rev, a2Pc} to delStack.
    //    - When pc is a2Pc, pop delStack.
    //
    // You might have noticed that we don't use the revs in LINE instructions
    // at all. This is because that LINE rev always matches its JL rev in this
    // implementation. In other words, the "rev" in LINE instruction is
    // redundant as it can be inferred from JL, with an insStack. Note in the
    // original C implementation of LineLog the LINE rev can be different from
    // the JL rev, to deal with merges while maintaining a linear history.
    const insStack: Frame[] = [{rev: 0, endPc: -1}];
    const delStack: Frame[] = [];
    const {onPc, onLine, onConditionalJump, onStackPush, onStackPop} = withContext(
      insStack,
      delStack,
    );
    let pc = 0;
    let patience = this.getSize() * 2;
    while (patience > 0) {
      onPc?.(pc);
      if (insStack.at(-1)?.endPc === pc) {
        insStack.pop();
        onStackPop?.(insStack);
      }
      if (delStack.at(-1)?.endPc === pc) {
        delStack.pop();
        onStackPop?.(delStack);
      }
      const code = unwrap(this.get(pc));
      switch (code.op) {
        case Op.LINE:
          onLine?.(code);
          pc += 1;
          break;
        case Op.END:
          patience = -1;
          break;
        case Op.J:
          pc = code.pc;
          break;
        case Op.JGE:
          onConditionalJump?.(code);
          delStack.push({rev: code.rev, endPc: code.pc});
          onStackPush?.(delStack);
          pc += 1;
          break;
        case Op.JL:
          onConditionalJump?.(code);
          insStack.push({rev: code.rev, endPc: code.pc});
          onStackPush?.(insStack);
          pc += 1;
          break;
        default:
          assert(false, 'bug: unknown code');
      }
      patience -= 1;
    }
    if (patience === 0) {
      assert(false, 'bug: code does not end in time');
    }
  }

  remapRevs(revMap: Map<Rev, Rev>): [Code, Rev] {
    let newMaxRev = 0;
    const newInstList = this.instList
      .map(c => {
        if (c.op === Op.JGE || c.op === Op.JL || c.op === Op.LINE) {
          const newRev = revMap.get(c.rev) ?? c.rev;
          if (newRev > newMaxRev) {
            newMaxRev = newRev;
          }
          // TypeScript cannot prove `c` has `rev`. Ideally it can figure out it automatically.
          return (c as RecordOf<{rev: number}>).set('rev', newRev) as Inst;
        }
        return c;
      })
      .toList();
    const newValueOf = `R${[...revMap.entries()]}`;
    const newCode = this.newCode(newInstList, newValueOf);
    return [newCode, newMaxRev];
  }

  private newCode(instList: List<Inst>, newValueOf: string): Code {
    const newStr = this.__valueOf + '\0' + newValueOf;
    // We want bitwise operations.
    // eslint-disable-next-line no-bitwise
    const newHash = (this.__hash * 23 + hash(newValueOf)) & 0x7fffffff;
    return new Code(instList, newHash, newStr);
  }
}

// Export for testing purpose.
export const executeCache: LRUWithStats = new LRU(100);

type LineLogProps = {
  /** Core state: instructions. The array index type is `Pc`. */
  code: Code;
  /** Maximum rev tracked (inclusive). */
  maxRev: Rev;
};

const LineLogRecord = Record<LineLogProps>({
  code: new Code(),
  maxRev: 0 as Rev,
});
type LineLogRecord = RecordOf<LineLogProps>;

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
class LineLog extends SelfUpdate<LineLogRecord> {
  constructor(props?: {code?: Code; maxRev?: Rev}) {
    const record = LineLogRecord(props);
    super(record);
  }

  get maxRev(): Rev {
    return this.inner.maxRev;
  }

  get code(): Code {
    return this.inner.code;
  }

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
   *
   * If `blockShift` is `true`, consider shifting the insertion lines
   * to relax dependency for easier reordering. Check the comments
   * in this function for details.
   */
  editChunk(
    aRev: Rev,
    a1: LineIdx,
    a2: LineIdx,
    bRev: Rev,
    bLines: string[],
    aLinesCache?: LineInfo[],
    blockShift = true,
  ): LineLog {
    const aLinesMutable = aLinesCache != null;
    const aLinesInfo: [LineInfo[], true] | [Readonly<LineInfo[]>, false] = aLinesMutable
      ? [aLinesCache, true]
      : [this.checkOutLines(aRev), false];

    const bLen = bLines.length;
    if (a1 === a2 && bLen > 0 && blockShift) {
      // Attempt to shift the insertion chunk so the start of insertion aligns
      // with another "start insertion". This might trigger the [OPT1]
      // optimization in `code.editChunk`, avoid nested insertions and enable
      // more flexible reordering.
      //
      // For example, we might get "Insert (rev 3)" below that forces a nested
      // insertion block. However, if we shift the block and use the
      // "Alternative Insert (rev 3)", we can use the [OPT1] optimization.
      //
      //   +----Insert (rev 1)
      //   |    Line:  function a () {
      //   |    Line:    return 'a';
      //   |    Line:  }
      //   +----
      //   +----Insert (rev 2)
      //   |                           ----+ Alternative Insert (rev 3)
      //   |    Line:                      |
      //   |+---Insert (rev 3)             |
      //   ||   Line:  function b () {     |
      //   ||   Line:    return 'b';       |
      //   ||   Line:  }                   |
      //   ||                          ----+
      //   ||   Line:
      //   |+---
      //   |    Line:  function c () {
      //   |    Line:    return 'c';
      //   |    Line:  }
      //   +----
      //
      // Block shifting works if the surrounding lines match, see:
      //
      //     A                                    A
      //     B                                  +-------+
      //   +-------+     is equivalent to       | B     |
      //   | block |     === shift up   ==>     | block |
      //   | B     |     <== shift down ===     +-------+
      //   +-------+                              B
      //     C                                    C

      const aLines: Readonly<LineInfo[]> = aLinesInfo[0];
      const canUseOpt1 = (a: LineIdx): boolean => {
        const pc = aLines.at(a)?.pc;
        // Check [OPT1] for how this works.
        return pc != null && pc > 0 && this.code.get(pc - 1)?.op === Op.JL;
      };
      if (!canUseOpt1(a1)) {
        const considerShift = (step: 'down' | 'up'): LineLog | undefined => {
          let ai = a1;
          let lines = [...bLines];
          // Limit overhead.
          const threshold = 10;
          for (let i = 0; i < threshold; ++i) {
            // Out of range?
            if (step === 'up' ? ai === 0 : ai === aLines.length - 1) {
              return undefined;
            }
            // Surrounding lines match?
            const [aIdx, bIdx] = step === 'up' ? [ai - 1, -1] : [ai, 0];
            const aData = aLines.at(aIdx)?.data;
            const bData = lines.at(bIdx);
            if (bData !== aData || bData == null) {
              return undefined;
            }
            // Shift.
            lines =
              step === 'up' ? [bData].concat(lines.slice(0, -1)) : lines.slice(1).concat([bData]);
            ai += step === 'up' ? -1 : 1;
            // Good enough?
            if (canUseOpt1(ai)) {
              return this.editChunk(aRev, ai, ai, bRev, lines, aLinesCache, false);
            }
          }
        };
        const maybeShifted = considerShift('up') ?? considerShift('down');
        if (maybeShifted != null) {
          return maybeShifted;
        }
      }
    }
    const newCode = this.code.editChunk(aRev, a1, a2, bRev, bLines, aLinesInfo);
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
    const [newCode, newMaxRev] = this.code.remapRevs(revMap);
    return new LineLog({code: newCode, maxRev: newMaxRev});
  }

  /**
   * Calculate the dependencies of revisions.
   * For example, `{5: [3, 1]}` means rev 5 depends on rev 3 and rev 1.
   *
   * Based on LineLog, which could be different from traditional textual
   * context-line dependencies. LineLog dependency is to prevent
   * "malformed cases" [1] when following the dependency to `remapRevs`.
   * Practically, LineLog might allow reorder cases that would be
   * disallowed by traditional context-line dependencies. See tests
   * for examples.
   *
   * [1]: Malformed cases are when nested blocks (insertions or deletions)
   *      might be skipped incorrectly. The outer block says "skip" and the
   *      inner block does not want to "skip" but is still skipped since it
   *      is skipped altogher with the outer block. See also section 0.4
   *      and 0.5 in D3628440.
   */
  @cached({cacheSize: 1000})
  calculateDepMap(): Readonly<Map<Rev, Set<Rev>>> {
    // With the insertion and deletion stacks (see explanation in
    // visitWithInsDelStacks), when we see a new insertion block, or deletion
    // block, we add two dependencies:
    // - The inner rev depends on the outer insertion rev.
    // - The outer deletion rev (if present) depends on the inner rev.
    //
    // Let's look at how this is done at the instruction level.
    // the instructions generated by editChunk:
    //
    //      a2Pc: ...
    //            ...
    //     start: JL brev b2Pc
    //            ...
    //      b2Pc: JGE brev a2Pc
    //          : <a1 Inst>
    //       end: J a1Pc+1
    //
    // JL is used for insertion, JGE is used for deletion. We then use them to
    // manipulate the insStack and delStack:
    //
    // insStack:
    //
    //    - On "start: JL brev b2Pc":
    //      Do not follow the JL jump. (by visitWithInsDelStacks)
    //      Mark brev as dependent on the outer insertion.
    //      Mark the outer deletion as dependent on this brev.
    //      Push {rev, b2Pc} to insStack. (by visitWithInsDelStacks)
    //    - When pc is b2Pc, pop insStack. (by visitWithInsDelStacks)
    //
    // delStack:
    //
    //    - On "b2Pc: JGE brev a2Pc":
    //      Do not follow the JGE jump. (by visitWithInsDelStacks)
    //      Mark brev as dependent on the outer insertion.
    //      Mark the outer deletion as dependent on this brev.
    //      Push {rev, a2Pc} to delStack. (by visitWithInsDelStacks)
    //    - When pc is a2Pc, pop delStack. (by visitWithInsDelStacks)
    const depMap = new Map<Rev, Set<Rev>>();
    const addDep = (child: Rev, parent: Rev) => {
      if (child > parent) {
        if (!depMap.has(child)) {
          depMap.set(child, new Set());
        }
        depMap.get(child)?.add(parent);
      }
    };
    this.code.visitWithInsDelStacks((insStack, delStack) => {
      const markDep = (rev: Rev) => {
        const ins = insStack.at(-1);
        if (ins !== undefined) {
          addDep(rev, ins.rev);
        }
        const del = delStack.at(-1);
        if (del !== undefined) {
          addDep(del.rev, rev);
        }
      };
      return {
        onConditionalJump: inst => markDep(inst.rev),
      };
    });
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
    let patience = this.code.getSize() * 2;
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
   */
  @cached({cacheSize: 1000})
  public flatten(): List<FlattenLine> {
    const result: FlattenLine[] = [];

    // See the comments in calculateDepMap for what the stacks mean.
    //
    // The flatten algorithm works as follows:
    // - For each line, we got an insRev (insStack.at(-1).rev), and a
    //   delRev (delStack.at(-1)?.rev ?? maxRev + 1), meaning the rev
    //   attached to the innermost insertion or deletion blocks,
    //   respectively.
    // - That line is then present in insRev .. delRev (exclusive) revs.
    //
    // This works because:
    // - The blocks are nested in order:
    //    - For nested insertions, the nested one must have a larger rev, and
    //      lines inside the nested block are only present starting from the
    //      larger rev.
    //    - For nested deletions, the nested one must have a smaller rev, and
    //      lines inside the nested block are considered as deleted by the
    //      smaller rev.
    //    - For interleaved insertion and deletions, insertion rev and deletion
    //      rev are tracked separately so their calculations are independent
    //      from each other.
    // - Linelog tracks linear history, so (insRev, delRev) can be converted to
    //   a Set<Rev>.
    this.code.visitWithInsDelStacks((insStack, delStack) => {
      const maxDelRev = this.maxRev + 1;
      const getCurrentRevs = (): ImSet<Rev> => {
        const insRev = insStack.at(-1)?.rev ?? 0;
        const delRev = delStack.at(-1)?.rev ?? maxDelRev;
        return revRangeToSet(insRev, delRev);
      };
      let currentRevs = getCurrentRevs();
      return {
        onStackPush: () => {
          currentRevs = getCurrentRevs();
        },
        onStackPop: () => {
          currentRevs = getCurrentRevs();
        },
        onLine: ({data}) => {
          result.push(FlattenLine({data, revs: currentRevs}));
        },
      };
    });
    return List(result);
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
  @cached({cacheSize: 1000})
  public recordText(text: string, rev: Rev | null = null): LineLog {
    // rev to edit from, and rev to match 'text'.
    const [aRev, bRev] = rev != null ? [rev, rev] : [this.maxRev, this.maxRev + 1];
    const b = text;

    const aLineInfos = [...this.checkOutLines(aRev)];
    const bLines = splitLines(b);
    const aLines = aLineInfos.map(l => l.data);
    aLines.pop(); // Drop the last END empty line.
    const blocks = diffLines(aLines, bLines);
    // eslint-disable-next-line @typescript-eslint/no-this-alias
    let log: LineLog = this;

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

/** Turn (3, 6) to Set([3, 4, 5]). */
const revRangeToSet = cached(
  (startRev, endRev: Rev): ImSet<Rev> => {
    const result: Rev[] = [];
    for (let rev = startRev; rev < endRev; rev++) {
      result.push(rev);
    }
    return ImSet(result);
  },
  {cacheSize: 1000},
);

function describeInst(inst: Inst): string {
  switch (inst.op) {
    case Op.J:
      return `J ${inst.pc}`;
    case Op.JGE:
      return `JGE ${inst.rev} ${inst.pc}`;
    case Op.JL:
      return `JL ${inst.rev} ${inst.pc}`;
    case Op.LINE:
      return `LINE ${inst.rev} ${JSON.stringify(inst.data.trimEnd())}`;
    case Op.END:
      return 'END';
  }
}

export {LineLog, FlattenLine};
export type {Rev, LineIdx, LineInfo};
