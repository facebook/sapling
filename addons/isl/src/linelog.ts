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

import {diff_match_patch} from 'diff-match-patch';

const dmp = new diff_match_patch();

enum Op {
  J = 0,
  JGE = 1,
  JL = 2,
  LINE = 3,
  END = 4,
}

interface J {
  op: Op.J;
  pc: Pc;
}

interface JGE {
  op: Op.JGE;
  rev: Rev;
  pc: Pc;
}

interface JL {
  op: Op.JL;
  rev: Rev;
  pc: Pc;
}

interface LINE {
  op: Op.LINE;
  rev: Rev;
  data: string;
}

interface END {
  op: Op.END;
}

type Pc = number;
type Rev = number;
type LineIdx = number;
type Inst = J | JGE | JL | LINE | END;

interface LineInfo {
  data: string;
  rev: Rev;
  pc: Pc;
  deleted: boolean;
}

class LineLog {
  // core state
  private code: Inst[];

  // cached states
  maxRev: Rev;
  private lastCheckoutKey: string;
  lines: LineInfo[];
  content: string;

  constructor() {
    this.code = [{op: Op.END}];
    this.maxRev = 0;
    this.lastCheckoutKey = '';
    this.lines = [];
    this.content = '';
    this.checkOut(0);
  }

  private editChunk(a1: LineIdx, a2: LineIdx, rev: Rev, lines: string[]) {
    assert(a1 <= a2, 'illegal chunk (a1 < a2)');
    assert(a2 <= this.lines.length, 'out of bound a2 (forgot checkOut?)');

    const start = this.code.length;
    const a1Pc = this.lines[a1].pc;
    if (lines.length > 0) {
      const b2Pc = start + lines.length + 1;
      this.code.push({op: Op.JL, rev, pc: b2Pc});
      lines.forEach(line => {
        this.code.push({op: Op.LINE, rev, data: line});
      });
      assert(b2Pc === this.code.length, 'bug: wrong pc');
    }
    if (a1 < a2) {
      const a2Pc = this.lines[a2 - 1].pc + 1;
      this.code.push({op: Op.JGE, rev, pc: a2Pc});
    }
    this.lines[a1].pc = this.code.length;
    this.code.push({...this.code[a1Pc]});
    switch (this.code[a1Pc].op) {
      case Op.J:
      case Op.END:
        break;
      default:
        this.code.push({op: Op.J, pc: a1Pc + 1});
    }
    this.code[a1Pc] = {op: Op.J, pc: start};

    const newLines = lines.map((s, i) => {
      return {data: s, rev, pc: start + 1 + i, deleted: false};
    });
    this.lines.splice(a1, a2 - a1, ...newLines);
    if (rev > this.maxRev) {
      this.maxRev = rev;
    }
    // NOTE: this.content is not updated here. It should be updated by the call-site.
  }

  private execute(
    startRev: Rev,
    endRev: Rev,
    present: {[pc: number]: boolean} | null = null,
  ): LineInfo[] {
    const rev = endRev;
    const lines: LineInfo[] = [];
    let pc = 0;
    let patience = this.code.length * 2;
    const deleted = present === null ? () => false : (pc: Pc) => !present[pc];
    while (patience > 0) {
      const code = this.code[pc];
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

  public checkOut(rev: Rev, start: Rev | null = null) {
    // eslint-disable-next-line no-param-reassign
    rev = Math.min(rev, this.maxRev);
    const key = `${rev},${start}`;
    if (key === this.lastCheckoutKey) {
      return;
    }

    let lines = this.execute(rev, rev);
    if (start !== null) {
      // Checkout a range, including deleted revs.
      const present: {[key: number]: boolean} = {};
      lines.forEach(l => {
        present[l.pc] = true;
      });

      // Go through all lines again. But do not skip chunks.
      lines = this.execute(start, rev, present);
    }

    this.lines = lines;
    this.content = this.reconstructContent();
    this.lastCheckoutKey = key;
  }

  private reconstructContent(): string {
    return this.lines.map(l => l.data).join('');
  }

  public recordText(text: string): Rev {
    const a = this.content;
    const b = text;

    const lines = splitLines(b);
    this.checkOut(this.maxRev);
    const blocks = diffLines(a, b);

    this.maxRev += 1;
    const rev = this.maxRev;
    blocks.reverse().forEach(([a1, a2, b1, b2]) => {
      this.editChunk(a1, a2, rev, lines.slice(b1, b2));
    });
    this.content = b;
    this.lastCheckoutKey = `${rev},null`;

    // assert(this.reconstructContent() === b, "bug: text does not match");
    return rev;
  }

  /** Get revision of the specified line. Returns null if out of range. */
  public getLineRev(i: LineIdx): Rev | null {
    if (i >= this.lines.length - 1) {
      return null;
    } else {
      return this.lines[i].rev;
    }
  }
}

function diffLines(a: string, b: string): [LineIdx, LineIdx, LineIdx, LineIdx][] {
  const {chars1, chars2} = dmp.diff_linesToChars_(a, b);
  const blocks: [LineIdx, LineIdx, LineIdx, LineIdx][] = [];
  let a1 = 0,
    a2 = 0,
    b1 = 0,
    b2 = 0;
  const push = (len: number) => {
    if (a1 !== a2 || b1 !== b2) {
      blocks.push([a1, a2, b1, b2]);
    }
    a1 = a2 = a2 + len;
    b1 = b2 = b2 + len;
  };
  dmp.diff_main(chars1, chars2, false).forEach(x => {
    const [op, chars] = x;
    const len = chars.length;
    if (op === 0) {
      push(len);
    }
    if (op < 0) {
      a2 += len;
    }
    if (op > 0) {
      b2 += len;
    }
  });
  push(0);
  return blocks;
}

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

function assert(condition: boolean, message: string) {
  if (!condition) {
    throw new Error(message);
  }
}

export {LineLog};
