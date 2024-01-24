/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {FlattenLine, LineIdx} from '../linelog';
import type {RecordOf} from 'immutable';

import {FileStackState} from './fileStackState';
import {List, Record, Set as ImSet} from 'immutable';
import {cached} from 'shared/LRU';
import {SelfUpdate} from 'shared/immutableExt';

/**
 * Represents selections of changes between 2 texts (`a` and `b`), optionally
 * the selection text can be edited free-form.
 *
 * Based on a 3-rev `FlattenLine`s representation:
 * - Rev 0: The `a` side (not editable by this `ChunkSelectState`).
 * - Rev 1: The selection (editable by this `ChunkSelectState`).
 * - Rev 2: The `b` side (not editable by this `ChunkSelectState`).
 *
 * Support operations:
 * - getLines: Obtain lines for rendering. See `SelectLine` for details.
 * - setSelectedLines: Set line selections. Only added or removed lines can be selected.
 * - getSelectedText: Obtain selected or edited text.
 * - setSelectedText: Set edited text. Useful for free-form editing.
 *
 * With free-from editing, there are two "special" cases:
 *
 *   revs    | meaning
 *   -----------------------------
 *   {0,1,2} | unchanged
 *   {0,1}   | deletion; not selected
 *   {0,2}   | special: extra deletion [*]
 *   {0}     | deletion; selected
 *   {1,2}   | insertion; selected
 *   {1}     | special: extra insertion
 *   {2}     | insertion; not selected
 *
 *   [*]: LineLog.flatten() never produces {0,2}. It generates {0} and {2} as
 *   separate lines. But `getLines()` post-processing might produce {0,2} lines.
 *
 * The callsite might want to treat the "special: extra insertion" like an
 * insertion with a different highlighting.
 *
 * This state does not care about collapsing unmodified lines, "context lines"
 * or "code expansion" states. They do not affect editing state, and belong to
 * the UI components.
 */
export class ChunkSelectState extends SelfUpdate<ChunkSelectRecord> {
  /**
   * Initialize ChunkSelectState from text `a` and `b`.
   *
   * If `selected` is `true`, then all changes are selected, selection result is `b`.
   * If `selected` is `false`, none of the changes are selected, selection result is `a`.
   * If `selected` is a string, then the selections are "inferred" from the string.
   *
   * If `normalize` is `true`, drop changes in `selected` that is not in `a` or `b`.
   */
  static fromText(
    a: string,
    b: string,
    selected: boolean | string,
    normalize = false,
  ): ChunkSelectState {
    const mid = selected === true ? b : selected === false ? a : selected;
    const fileStack = new FileStackState([a, mid, b]);
    let lines = fileStack.convertToFlattenLines().map(l => toLineBits(l));
    if (normalize) {
      lines = lines.filter(l => l.bits !== 0b101 && l.bits !== 0b010 && l.bits !== 0b000);
    }
    return new ChunkSelectState(ChunkSelectRecord({a, b, lines}));
  }

  /** Get the text of the "a" side. */
  get a(): string {
    return this.inner.a;
  }

  /** Get the text of the "b" side. */
  get b(): string {
    return this.inner.b;
  }

  /**
   * Get the `SelectLine`s. Useful for rendering the lines.
   * See `SelectLine` for details.
   */
  @cached()
  getLines(): Readonly<SelectLine[]> {
    let nextALine = 1;
    let nextBLine = 1;
    let nextSelLine = 1;
    let result: SelectLine[] = [];

    // Modified lines to sort before appending to `result`.
    let buffer: SelectLine[] = [];
    const pushBuffer = () => {
      buffer.sort((a, b) => {
        // In this order: Deletion, insertion.
        const aOrder = bitsToOrder[a.bits];
        const bOrder = bitsToOrder[b.bits];
        if (aOrder !== bOrder) {
          return aOrder - bOrder;
        }
        return a.rawIndex - b.rawIndex;
      });
      // Merge "selected deletion + deselected insertion" with
      // the same content into "unselectable deletion".
      let nextDelIndex = 0;
      buffer.forEach((line, i) => {
        if (line.bits === 0b001 /* deselected insertion */) {
          // Try to find the matched "selected deletion" line.
          while (nextDelIndex < i) {
            const otherLine = buffer[nextDelIndex];
            if (otherLine.data === line.data && otherLine.bits === 0b100) {
              // Change otherLine to "unselectable deletion",
              // then remove this line.
              otherLine.bits = 0b101;
              otherLine.selected = null;
              otherLine.bLine = line.bLine;
              otherLine.sign = '!-';
              line.bits = 0;
              break;
            }
            nextDelIndex += 1;
          }
        }
      });
      buffer = buffer.filter(line => line.bits !== 0);
      result = result.concat(buffer);
      buffer = [];
    };

    this.inner.lines.forEach((line, rawIndex) => {
      const bits = line.bits;
      let sign: Sign = '';
      let selected: boolean | null = null;
      let aLine: LineIdx | null = null;
      let bLine: LineIdx | null = null;
      let selLine: LineIdx | null = null;
      // eslint-disable-next-line no-bitwise
      if (bits >> 2 !== 0) {
        aLine = nextALine;
        nextALine += 1;
      }
      // eslint-disable-next-line no-bitwise
      if ((bits & 1) !== 0) {
        bLine = nextBLine;
        nextBLine += 1;
      }
      // eslint-disable-next-line no-bitwise
      if ((bits & 2) !== 0) {
        selLine = nextSelLine;
        nextSelLine += 1;
      }
      switch (bits) {
        case 0b001:
          sign = '+';
          selected = false;
          break;
        case 0b010:
          sign = '!+';
          break;
        case 0b011:
          sign = '+';
          selected = true;
          break;
        case 0b100:
          sign = '-';
          selected = true;
          break;
        case 0b101:
          sign = '!-';
          break;
        case 0b110:
          sign = '-';
          selected = false;
          break;
        case 0b111:
          break;
      }
      const selectLine: SelectLine = {
        rawIndex,
        aLine,
        bLine,
        selLine,
        sign,
        selected,
        bits,
        data: line.data,
      };
      if (sign === '') {
        pushBuffer();
        result.push(selectLine);
      } else {
        buffer.push(selectLine);
      }
    });
    pushBuffer();
    return result;
  }

  /**
   * Get the line regions. By default, unchanged lines are collapsed.
   *
   * `config.contextLines` sets how many lines to expand around
   * changed or current lines.
   *
   * `config.expanded` and `config.caretLine` specify lines to
   * expanded.
   */
  getLineRegions(config?: {
    contextLines?: number;
    /** Line numbers on the "A" side to expand. */
    expandedALines: ImSet<number>;
    /** Line number on the "M" (selection) side to expand. */
    expandedSelLine?: number;
  }): Readonly<LineRegion[]> {
    const contextLines = config?.contextLines ?? 2;
    const lines = this.getLines();
    const expandedSelLine = config?.expandedSelLine ?? -1;
    const expandedALines = config?.expandedALines ?? ImSet();
    const regions: LineRegion[] = [];

    // Figure out indexes of `lines` to collapse (skip).
    const collapsedLines = Array<boolean>(lines.length + contextLines).fill(true);
    lines.forEach((line, i) => {
      if (
        line.bits !== 0b111 ||
        expandedALines.has(line.aLine ?? -1) ||
        line.selLine === expandedSelLine
      ) {
        for (let j = i + contextLines; j >= 0 && j >= i - contextLines && collapsedLines[j]; j--) {
          collapsedLines[j] = false;
        }
      }
    });

    // Scan through regions.
    let currentRegion: LineRegion | null = null;
    lines.forEach((line, i) => {
      const same = line.bits === 0b111;
      const collapsed = collapsedLines[i];
      if (currentRegion?.same === same && currentRegion?.collapsed === collapsed) {
        currentRegion.lines.push(line);
      } else {
        if (currentRegion !== null) {
          regions.push(currentRegion);
        }
        currentRegion = {lines: [line], same, collapsed};
      }
    });
    if (currentRegion !== null) {
      regions.push(currentRegion);
    }

    return regions;
  }

  /**
   * Get the text of selected lines. It is the editing result.
   *
   * Note: passing `getSelectedText` to `fromText` does not maintain the selection
   * state. For example, from an empty text to `1\n1\n1\n`. The user might select
   * the 1st, 2nd, or 3rd line. That's 3 different ways of selections with the same
   * `getSelectedText` output.
   */
  @cached()
  getSelectedText(): string {
    return (
      this.inner.lines
        // eslint-disable-next-line no-bitwise
        .filter(l => (l.bits & 0b010) !== 0)
        .map(l => l.data)
        .join('')
    );
  }

  /**
   * Calculate the "inverse" of selected text. Useful for `revert -i` or "Discard".
   *
   * A Selected B | Inverse Note
   * 0        0 1 | 1       + not selected, preserve B
   * 0        1 0 | 0       = preserve B
   * 0        1 1 | 0       + selected, drop B, preserve A
   * 1        0 0 | 1       - selected, drop B, preserve A
   * 1        0 1 | 1       = preserve B
   * 1        1 0 | 0       - not selected, preserve B
   * 1        1 1 | 1       = preserve B
   */
  getInverseText(): string {
    return this.inner.lines
      .filter(l => [0b001, 0b100, 0b101, 0b111].includes(l.bits))
      .map(l => l.data)
      .join('');
  }

  /**
   * Select or deselect lines.
   *
   * `selects` is a list of tuples. Each tuple has a `rawIndex` and whether that
   * line is selected or not.
   * Note if a line is deleted (sign is '-'), then selected means deleting that line.
   *
   * Note all lines are editable. Lines that are not editable are silently ignored.
   */
  setSelectedLines(selects: Array<[LineIdx, boolean]>): ChunkSelectState {
    const newLines = this.inner.lines.withMutations(mutLines => {
      let lines = mutLines;
      selects.forEach(([idx, selected]) => {
        const line = lines.get(idx);
        if (line === undefined) {
          return;
        }
        const {bits} = line;
        // eslint-disable-next-line no-bitwise
        const bits101 = bits & 0b101;
        if (bits101 === 0 || bits101 === 0b101) {
          // Not changed in v0 and v2 - ignore editing.
          return;
        }
        const oldSelected: boolean =
          // eslint-disable-next-line no-bitwise
          bits101 === 0b100 ? (bits & 0b010) === 0 : (bits & 0b010) !== 0;
        if (oldSelected !== selected) {
          // Update selection by toggling (xor) rev 1.
          // eslint-disable-next-line no-bitwise
          const newBits = bits ^ 0b010;
          const newLine = line.set('bits', newBits as Bits);
          lines = lines.set(idx, newLine);
        }
      });
      return lines;
    });
    return new ChunkSelectState(this.inner.set('lines', newLines));
  }

  /**
   * Free-form edit selected text.
   *
   * Runs analysis to mark lines as selected. Consider only calling this once
   * when switching from free-form editing to line selections.
   */
  setSelectedText(text: string): ChunkSelectState {
    const {a, b} = this.inner;
    return ChunkSelectState.fromText(a, b, text);
  }

  /**
   * The constructor is for internal use only.
   * Use static methods to construct `ChunkSelectState`.
   */
  constructor(record: ChunkSelectRecord) {
    super(record);
  }
}

/** A line and its position on both sides, and selection state. */
export type SelectLine = {
  /** Index in the `lines` internal state. Starting from 0. */
  rawIndex: LineIdx;

  /** Line index on the `a` side for rendering, or `null` if the line does not exist on the `a` side. */
  aLine: LineIdx | null;

  /** Line index on the `b` side for rendering, or `null` if the line does not exist on the `b` side. */
  bLine: LineIdx | null;

  /** Line index for "selected" lines, for rendering, or `null` if the line is not in the "selected" side. */
  selLine: LineIdx | null;

  /** See `Sign` for description. */
  sign: Sign;

  /**
   * Whether the line is selected or not. Only used when sign is '-' or '+'.
   * Note if a line is deleted (sign is '-'), then selected means deleting that line.
   */
  selected: boolean | null;

  /** Line selection bits. */
  bits: Bits;

  /** Content of the line. */
  data: string;
};

/**
 * A contiguous range of lines that share same properties.
 * Properties include: "same for all version", "collapsed".
 */
export type LineRegion = {
  /** Lines in the region. */
  lines: SelectLine[];

  /** If the region has the same content for all versions. */
  same: boolean;

  /** If the region is collapsed. */
  collapsed: boolean;
};

/** '-': deletion; '+': insertion; '': unchanged; '!+', '!-': forced insertion or deletion, not selectable. */
type Sign = '' | '+' | '-' | '!+' | '!-';

type ChunkSelectProps = {
  a: string;
  b: string;
  lines: List<LineBitsRecord>;
};
const ChunkSelectRecord = Record<ChunkSelectProps>({a: '', b: '', lines: List()});
type ChunkSelectRecord = RecordOf<ChunkSelectProps>;

type Bits = 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7;

/** Similar to `FlattenLine` but compress Set<Rev> into 3 bits for easier access. */
type LineBitsProps = {
  /** Line content including `\n` EOL. */
  data: string;
  /** Bitset. 0b100: left side; 0b001: right side; 0b010: editing / selecting text. */
  bits: Bits;
};
const LineBitsRecord = Record<LineBitsProps>({data: '', bits: 0});
type LineBitsRecord = RecordOf<LineBitsProps>;

/**
 * Converts `FlattenLine` to `LineBits`.
 * `line.revs` (`ImSet<Rev>`) is converted to 3 bits. 0b100: rev 0; 0b010: rev 1; 0b001: rev 2
 */
function toLineBits(line: FlattenLine): LineBitsRecord {
  // eslint-disable-next-line no-bitwise
  const bits = line.revs.reduce((acc, rev) => acc | (4 >> rev), 0);
  return LineBitsRecord({data: line.data, bits: bits as Bits});
}

const bitsToOrder = [
  0, // 0b000: unused
  2, // 0b001: normal insertion
  2, // 0b010: unselectable insertion
  2, // 0b011: normal insertion
  1, // 0b100: normal deletion
  1, // 0b101: unselectable deletion
  1, // 0b110: normal deletion
  0, // 0b111: normal
];
