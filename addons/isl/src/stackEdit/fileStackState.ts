/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {FlattenLine, Rev, LineIdx} from '../linelog';

import {LineLog} from '../linelog';

/**
 * A stack of file contents with stack editing features.
 */
export class FileStackState {
  /** Source of truth that provides the file contents. */
  source: FileStackStateSource;

  /** Maximum revision (exclusive) */
  revLength: Rev;

  constructor(contents: string[]) {
    this.revLength = contents.length;
    this.source = {
      type: 'plain',
      contents: [...contents],
    };
  }

  // Read opertions.

  /**
   * Obtain the content at the given revision.
   * 0 <= rev < this.revLength
   */
  get(rev: Rev): string {
    const type = this.source?.type;
    if (type === 'linelog') {
      return this.source.log.checkOut(rev);
    } else if (type === 'flatten') {
      return this.source.lines
        .filter(l => l.revs.has(rev))
        .map(l => l.data)
        .join('');
    } else if (type === 'plain') {
      return this.source.contents[rev];
    }
    throw new Error(`unexpected source type ${type}`);
  }

  /** Array of valid revisions. */
  revs(): Rev[] {
    return [...Array(this.revLength).keys()];
  }

  /**
   * Calculate the dependencies of revisions.
   * For example, `{5: [3, 1]}` means rev 5 depends on rev 3 and rev 1.
   */
  calculateDepMap(): Map<Rev, Set<Rev>> {
    return this.convertToLineLog().calculateDepMap();
  }

  /** Figure out which `rev` introduces the lines. */
  blame(rev: Rev): Rev[] {
    const log = this.convertToLineLog();
    log.checkOut(rev);
    const revs: Rev[] = [];
    for (let i = 0; ; i++) {
      const rev = log.getLineRev(i);
      if (rev == null) {
        break;
      }
      revs.push(rev);
    }
    return revs;
  }

  // Write opertions.

  /**
   * Edit full text of a rev.
   * If `updateStack` is true, the rest of the stack will be updated
   * accordingly. Otherwise, no other revs are updated.
   */
  editText(rev: Rev, text: string, updateStack = true) {
    if (updateStack) {
      this.convertToLineLog().recordText(text, rev);
    } else {
      this.convertToPlainText()[rev] = text;
    }
    if (rev >= this.revLength) {
      this.revLength = rev + 1;
    }
  }

  /**
   * Replace line range `a1` to `a2` at `aRev` with `bLines` from `bRev`.
   * The rest of the stack will be updated accordingly.
   *
   * The `aRev` decides what `a1` and `a2` mean, since line indexes
   * from different revs are different. The `aRev` is not the rev that
   * makes the change.
   *
   * The `bRev` is the revision that makes the change (deletion, insertion).
   *
   * This is useful to implement absorb-like edits at chunk (not full text)
   * level. For example, absorb runs from the top of a stack, and calculates
   * the diff between the stack top commit and the current working copy.
   * So `aRev` is the stack top, since the diff uses line numbers in the
   * stack top. `bRev` is the revisions that each chunk blames to.
   */
  editChunk(aRev: Rev, a1: LineIdx, a2: LineIdx, bRev: Rev, bLines: string[]) {
    const log = this.convertToLineLog();
    log.checkOut(aRev);
    log.editChunk(a1, a2, bRev, bLines);
  }

  /**
   * Remap the revs. This can be useful for reordering, folding,
   * and insertion. The callsite is responsible for checking
   * `revDepMap` to ensure the reordering can be "conflict"-free.
   */
  remapRevs(revMap: Map<Rev, Rev>) {
    const log = this.convertToLineLog();
    log.remapRevs(revMap);
    this.revLength = log.maxRev + 1;
  }

  /**
   * Move (or copy) line range `a1` to `a2` at `aRev` to other revs.
   * Those lines will be included by `includeRevs` and excluded by `excludeRevs`.
   */
  moveLines(aRev: Rev, a1: LineIdx, a2: LineIdx, includeRevs?: Rev[], excludeRevs?: Rev[]) {
    const lines = this.convertToFlattenLines();
    const editLine = (line: FlattenLine) => {
      if (includeRevs) {
        includeRevs.forEach(rev => line.revs.add(rev));
      }
      if (excludeRevs) {
        excludeRevs.forEach(rev => line.revs.delete(rev));
      }
    };

    // Note `lineStart` and `lineEnd` are for lines in `rev`.
    // The indexes cannot be used by `lines`. We need to filter `lines` by `rev`.
    let revLineIdx = 0;
    for (let i = 0; i < lines.length; ++i) {
      const line = lines[i];
      if (line.revs.has(aRev)) {
        if (revLineIdx >= a1 && revLineIdx < a2) {
          editLine(line);
        }
        revLineIdx += 1;
        if (revLineIdx >= a2) {
          break;
        }
      }
    }
  }

  // Internal format convertions.

  /** Convert to LineLog representation on demand. */
  convertToLineLog(): LineLog {
    const type = 'linelog';
    if (this.source.type === type) {
      return this.source.log;
    }
    const log = new LineLog();
    this.revs().forEach(rev => {
      const data = this.get(rev);
      log.recordText(data, rev);
    });
    this.source = {type, log};
    return log;
  }

  /** Convert to flatten representation on demand. */
  convertToFlattenLines(): FlattenLine[] {
    const type = 'flatten';
    if (this.source.type === type) {
      return this.source.lines;
    }
    const log = this.convertToLineLog();
    const lines = log.flatten();
    this.source = {type, lines};
    return lines;
  }

  /** Convert to plain representation on demand. */
  convertToPlainText(): string[] {
    const type = 'plain';
    if (this.source.type === type) {
      return this.source.contents;
    }
    const contents = this.revs().map(this.get.bind(this));
    this.source = {type, contents};
    return contents;
  }
}

/**
 * Depending on the operation, there are different ways to represent the file contents:
 * - plain: Full text per revision. This is the initial representation.
 * - linelog: LineLog representation. This is useful for analysis (dependency, target
 *   revs for absorb, line number offset calculation, etc), certain kinds of
 *   editing, and generating the flatten representation.
 * - flatten: The flatten view of LineLog. This is useful for moving lines between
 *   revisioins more explicitly.
 */
type FileStackStateSource =
  | {
      type: 'linelog';
      log: LineLog;
    }
  | {
      type: 'plain';
      contents: string[];
    }
  | {
      type: 'flatten';
      lines: FlattenLine[];
    };

export type {Rev};
