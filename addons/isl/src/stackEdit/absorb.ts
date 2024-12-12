/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {LineInfo} from '../linelog';
import type {Rev} from './fileStackState';
import type {RecordOf} from 'immutable';

import {assert} from '../utils';
import {FileStackState} from './fileStackState';
import {List, Record} from 'immutable';
import {diffLines, splitLines} from 'shared/diff';
import {dedup, nullthrows} from 'shared/utils';

/** A diff chunk analyzed by `analyseFileStack`. */
export type AbsorbDiffChunkProps = {
  /** The start line of the old content (start from 0, inclusive). */
  oldStart: number;
  /** The end line of the old content (start from 0, exclusive). */
  oldEnd: number;
  /** The new content to replace the old content. */
  newLines: List<string>;
  /**
   * Which rev introduces the "old" chunk.
   * The following revs are expected to contain this chunk too.
   * This is usually the "blame" rev in the stack.
   */
  introductionRev: Rev;
  /**
   * File revision (starts from 0) that the diff chunk is currently
   * selected to apply to. `null`: no selectioin.
   * Initially, this is the "suggested" rev to absorb to. Later,
   * the user can change this to a different rev.
   * Must be >= introductionRev.
   */
  selectedRev: Rev | null;
};

export const AbsorbDiffChunk = Record<AbsorbDiffChunkProps>({
  oldStart: 0,
  oldEnd: 0,
  newLines: List(),
  introductionRev: 0,
  selectedRev: null,
});
export type AbsorbDiffChunk = RecordOf<AbsorbDiffChunkProps>;

/**
 * Given a stack and the latest changes (usually at the stack top),
 * calculate the diff chunks and the revs that they might be absorbed to.
 * The rev 0 of the file stack should come from a "public" (immutable) commit.
 */
export function analyseFileStack(stack: FileStackState, newText: string): List<AbsorbDiffChunk> {
  assert(stack.revLength > 0, 'stack should not be empty');
  const linelog = stack.convertToLineLog();
  const oldRev = stack.revLength - 1;
  const oldText = stack.getRev(oldRev);
  const oldLines = splitLines(oldText);
  // The `LineInfo` contains "blame" information.
  const oldLineInfos = linelog.checkOutLines(oldRev);
  const newLines = splitLines(newText);
  const result: Array<AbsorbDiffChunk> = [];
  diffLines(oldLines, newLines).forEach(([a1, a2, b1, b2]) => {
    // a1, a2: line numbers in the `oldRev`.
    // b1, b2: line numbers in `newText`.
    // See also [`_analysediffchunk`](https://github.com/facebook/sapling/blob/6f29531e83daa62d9bd3bc58b712755d34f41493/eden/scm/sapling/ext/absorb/__init__.py#L346)
    let involvedLineInfos = oldLineInfos.slice(a1, a2);
    if (involvedLineInfos.length === 0 && oldLineInfos.length > 0) {
      // This is an insertion. Check the surrounding lines, excluding lines from the public commit.
      const nearbyLineNumbers = dedup([a2, Math.max(0, a1 - 1)]);
      involvedLineInfos = nearbyLineNumbers.map(i => oldLineInfos[i]);
    }
    // Check the revs. Skip public commits. The Python implementation only skips public
    // for insertions. Here we aggressively skip public lines for modification and deletion too.
    const involvedRevs = dedup(involvedLineInfos.map(info => info.rev).filter(rev => rev > 0));
    if (involvedRevs.length === 1) {
      // Only one rev. Set selectedRev to this.
      // For simplicity, we're not checking the "continuous" lines here yet (different from Python).
      const introductionRev = involvedRevs[0];
      result.push(
        AbsorbDiffChunk({
          oldStart: a1,
          oldEnd: a2,
          newLines: List(newLines.slice(b1, b2)),
          introductionRev,
          selectedRev: introductionRev,
        }),
      );
    } else if (b1 === b2) {
      // Deletion. Break the chunk into sub-chunks with different selectedRevs.
      // For simplicity, we're not checking the "continuous" lines here yet (different from Python).
      splitChunk(a1, a2, oldLineInfos, (oldStart, oldEnd, introductionRev) => {
        result.push(
          AbsorbDiffChunk({
            oldStart,
            oldEnd,
            newLines: List([]),
            introductionRev,
            selectedRev: introductionRev,
          }),
        );
      });
    } else if (a2 - a1 === b2 - b1 && involvedLineInfos.every(info => info.rev > 0)) {
      // Line count matches on both side. No public lines.
      // We assume the "a" and "b" sides are 1:1 mapped.
      // So, even if the "a"-side lines blame to different revs, we can
      // still break the chunks to individual lines.
      const delta = b1 - a1;
      splitChunk(a1, a2, oldLineInfos, (oldStart, oldEnd, introductionRev) => {
        result.push(
          AbsorbDiffChunk({
            oldStart,
            oldEnd,
            newLines: List(newLines.slice(oldStart + delta, oldEnd + delta)),
            introductionRev,
            selectedRev: introductionRev,
          }),
        );
      });
    } else {
      // Other cases, like replacing 10 lines from 3 revs to 20 lines.
      // It might be possible to build extra fancy UIs to support it
      // asking the user which sub-chunk on the "a" side matches which
      // sub-chunk on the "b" side.
      // For now, we just report this chunk as a whole chunk that can
      // only be absorbed to the "max" rev where the left side is
      // "settled" down.
      result.push(
        AbsorbDiffChunk({
          oldStart: a1,
          oldEnd: a2,
          newLines: List(newLines.slice(b1, b2)),
          introductionRev: Math.max(0, ...involvedRevs),
          selectedRev: null,
        }),
      );
    }
  });
  return List(result);
}

/**
 * Apply edits specified by `chunks`.
 * Each `chunk` can specify which rev it wants to absorb to by setting `selectedRev`.
 */
export function applyFileStackEdits(
  stack: FileStackState,
  chunks: Iterable<AbsorbDiffChunk>,
): FileStackState {
  // See also [apply](https://github.com/facebook/sapling/blob/6f29531e83daa62d9bd3bc58b712755d34f41493/eden/scm/sapling/ext/absorb/__init__.py#L321)
  assert(stack.revLength > 0, 'stack should not be empty');
  let linelog = stack.convertToLineLog();
  // Remap revs from rev to rev * 2. So we can edit rev * 2 + 1 to override contents.
  linelog = linelog.remapRevs(new Map(Array.from({length: stack.revLength}, (_, i) => [i, i * 2])));
  const oldRev = stack.revLength - 1;
  // Apply the changes. Assuming there are no overlapping chunks, we apply
  // from end to start so the line numbers won't need change.
  const sortedChunks = [...chunks]
    .filter(c => c.selectedRev != null)
    .toSorted((a, b) => b.oldEnd - a.oldEnd);
  sortedChunks.forEach(chunk => {
    const targetRev = nullthrows(chunk.selectedRev);
    assert(
      targetRev >= chunk.introductionRev,
      `selectedRev ${targetRev} must be >= introductionRev ${chunk.introductionRev}`,
    );
    assert(
      targetRev > 0,
      'selectedRev must be > 0 since rev 0 is from the immutable public commit',
    );
    // Edit the content of a past revision (targetRev, and follow-ups) from a
    // future revision (oldRev, matches the line numbers).
    linelog = linelog.editChunk(
      oldRev * 2,
      chunk.oldStart,
      chunk.oldEnd,
      targetRev * 2 + 1,
      chunk.newLines.toArray(),
    );
  });
  const texts = Array.from({length: stack.revLength}, (_, i) => linelog.checkOut(i * 2 + 1));
  return new FileStackState(texts);
}

/** Split the start..end chunk into sub-chunks so each chunk has the same "blame" rev. */
function splitChunk(
  start: number,
  end: number,
  lineInfos: readonly LineInfo[],
  callback: (_start: number, _end: number, _introductionRev: Rev) => void,
) {
  let lastStart = start;
  for (let i = start; i < end; i++) {
    const introductionRev = lineInfos[i].rev;
    if (i + 1 === end || introductionRev != lineInfos[i + 1].rev) {
      callback(lastStart, i + 1, introductionRev);
      lastStart = i + 1;
    }
  }
}
