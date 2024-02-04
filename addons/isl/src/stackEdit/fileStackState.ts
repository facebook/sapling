/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {FlattenLine, Rev, LineIdx} from '../linelog';
import type {RecordOf} from 'immutable';

import {LineLog} from '../linelog';
import {Record, List} from 'immutable';
import {SelfUpdate} from 'shared/immutableExt';

/**
 * A stack of file contents with stack editing features.
 */
export class FileStackState extends SelfUpdate<FileStackStateRecord> {
  constructor(value: Source | string[]) {
    if (Array.isArray(value)) {
      const contents: string[] = value;
      const source = Source({type: 'plain', value: List(contents), revLength: contents.length});
      super(FileStackStateRecord({source}));
    } else {
      super(FileStackStateRecord({source: value}));
    }
  }

  get source(): Source {
    return this.inner.source;
  }

  get revLength(): number {
    return this.inner.source.revLength;
  }

  fromLineLog(log: LineLog): FileStackState {
    return new FileStackState(Source({type: 'linelog', value: log, revLength: log.maxRev + 1}));
  }

  fromFlattenLines(lines: List<FlattenLine>, revLength: number | undefined): FileStackState {
    const newRevLength = revLength ?? lines.map(l => l.revs.max()).max() ?? 0;
    const source = Source({type: 'flatten', value: lines, revLength: newRevLength});
    return new FileStackState(source);
  }

  // Read operations.

  /**
   * Obtain the content at the given revision.
   * 0 <= rev < this.revLength
   */
  getRev(rev: Rev): string {
    const type = this.source.type;
    if (type === 'linelog') {
      return this.source.value.checkOut(rev);
    } else if (type === 'flatten') {
      return this.source.value
        .filter(l => l.revs.has(rev))
        .map(l => l.data)
        .join('');
    } else if (type === 'plain') {
      return this.source.value.get(rev, '');
    }
    throw new Error(`unexpected source type ${type}`);
  }

  /** Array of valid revisions. */
  revs(): Rev[] {
    return [...Array(this.source.revLength).keys()];
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
    const lines = log.checkOutLines(rev);
    // Skip the last 'END' line.
    return lines.slice(0, lines.length - 1).map(l => l.rev);
  }

  // Write operations.

  /**
   * Edit full text of a rev.
   * If `updateStack` is true, the rest of the stack will be updated
   * accordingly. Otherwise, no other revs are updated.
   */
  editText(rev: Rev, text: string, updateStack = true): FileStackState {
    const revLength = rev >= this.source.revLength ? rev + 1 : this.source.revLength;
    let source = this.source;
    if (updateStack) {
      const log = this.convertToLineLog().recordText(text, rev);
      source = Source({type: 'linelog', value: log, revLength});
    } else {
      const plain = this.convertToPlainText().set(rev, text);
      source = Source({type: 'plain', value: plain, revLength});
    }
    return new FileStackState(source);
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
  editChunk(aRev: Rev, a1: LineIdx, a2: LineIdx, bRev: Rev, bLines: string[]): FileStackState {
    const log = this.convertToLineLog().editChunk(aRev, a1, a2, bRev, bLines);
    return this.fromLineLog(log);
  }

  /**
   * Remap the revs. This can be useful for reordering, folding,
   * and insertion. The callsite is responsible for checking
   * `revDepMap` to ensure the reordering can be "conflict"-free.
   */
  remapRevs(revMap: Map<Rev, Rev>): FileStackState {
    const log = this.convertToLineLog().remapRevs(revMap);
    return this.fromLineLog(log);
  }

  /**
   * Move (or copy) line range `a1` to `a2` at `aRev` to other revs.
   * Those lines will be included by `includeRevs` and excluded by `excludeRevs`.
   *
   * PERF: It would be better to just use linelog to complete the edit.
   */
  moveLines(
    aRev: Rev,
    a1: LineIdx,
    a2: LineIdx,
    includeRevs?: Rev[],
    excludeRevs?: Rev[],
  ): FileStackState {
    let revLineIdx = 0;
    const editLine = (line: FlattenLine): FlattenLine => {
      let newLine = line;
      if (line.revs.has(aRev)) {
        if (revLineIdx >= a1 && revLineIdx < a2) {
          const newRevs = line.revs.withMutations(mutRevs => {
            let revs = mutRevs;
            if (includeRevs) {
              revs = revs.union(includeRevs);
            }
            if (excludeRevs) {
              revs = revs.subtract(excludeRevs);
            }
            return revs;
          });
          newLine = line.set('revs', newRevs);
        }
        revLineIdx++;
      }
      return newLine;
    };

    return this.mapAllLines(editLine);
  }

  /**
   * Edit lines for all revisions using a callback.
   * The return type can be an array (like flatMap), to insert or delete lines.
   */
  mapAllLines(
    editLineFunc: (line: FlattenLine, i: number) => FlattenLine | FlattenLine[],
  ): FileStackState {
    const lines = this.convertToFlattenLines().flatMap((line, i) => {
      const mapped = editLineFunc(line, i);
      return Array.isArray(mapped) ? mapped : [mapped];
    });
    return this.fromFlattenLines(lines, this.revLength);
  }

  // Internal format conversions.

  /** Convert to LineLog representation on demand. */
  convertToLineLog(): LineLog {
    const type = 'linelog';
    if (this.source.type === type) {
      return this.source.value;
    }
    let log = new LineLog();
    this.revs().forEach(rev => {
      const data = this.getRev(rev);
      log = log.recordText(data, rev);
    });
    return log;
  }

  /** Convert to flatten representation on demand. */
  convertToFlattenLines(): List<FlattenLine> {
    const type = 'flatten';
    if (this.source.type === type) {
      return this.source.value;
    }
    const log = this.convertToLineLog();
    const lines = log.flatten();
    return List(lines);
  }

  /** Convert to plain representation on demand. */
  convertToPlainText(): List<string> {
    const type = 'plain';
    if (this.source.type === type) {
      return this.source.value;
    }
    const contents = this.revs().map(this.getRev.bind(this));
    return List(contents);
  }
}

/**
 * Depending on the operation, there are different ways to represent the file contents:
 * - plain: Full text per revision. This is the initial representation.
 * - linelog: LineLog representation. This is useful for analysis (dependency, target
 *   revs for absorb, line number offset calculation, etc), certain kinds of
 *   editing, and generating the flatten representation.
 * - flatten: The flatten view of LineLog. This is useful for moving lines between
 *   revisions more explicitly.
 */
type SourceProps =
  | {
      type: 'linelog';
      value: LineLog;
      revLength: number;
    }
  | {
      type: 'plain';
      value: List<string>;
      revLength: number;
    }
  | {
      type: 'flatten';
      value: List<FlattenLine>;
      revLength: number;
    };

export const Source = Record<SourceProps>({
  type: 'plain',
  value: List([]),
  revLength: 0,
});
type Source = RecordOf<SourceProps>;

type FileStackStateProps = {
  source: Source;
};
const FileStackStateRecord = Record<FileStackStateProps>({source: Source()});
type FileStackStateRecord = RecordOf<FileStackStateProps>;

export type {Rev};
