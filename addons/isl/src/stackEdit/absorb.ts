/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RecordOf} from 'immutable';
import type {LineInfo} from '../linelog';
import type {FileStackIndex} from './commitStackState';
import type {FileRev, FileStackState} from './fileStackState';

import {Map as ImMap, List, Record} from 'immutable';
import {diffLines, splitLines} from 'shared/diff';
import {dedup, nullthrows} from 'shared/utils';
import {t} from '../i18n';
import {assert} from '../utils';
import {max, prev} from './revMath';

/** A diff chunk analyzed by `analyseFileStack`. */
export type AbsorbEditProps = {
  /** The start line of the old content (start from 0, inclusive). */
  oldStart: number;
  /** The end line of the old content (start from 0, exclusive). */
  oldEnd: number;
  /**
   * The old content to be replaced by newLines.
   * If you know the full content of the old file as `allLines`, you
   * can also use `allLines.slice(oldStart, oldEnd)` to get this.
   */
  oldLines: List<string>;
  /** The start line of the new content (start from 0, inclusive). */
  newStart: number;
  /** The end line of the new content (start from 0, exclusive). */
  newEnd: number;
  /** The new content to replace oldLines. */
  newLines: List<string>;
  /**
   * Which rev introduces the "old" chunk.
   * The following revs are expected to contain this chunk too.
   * This is usually the "blame" rev in the stack.
   */
  introductionRev: FileRev;
  /**
   * File revision (starts from 0) that the diff chunk is currently
   * selected to apply to. `null`: no selectioin.
   * Initially, this is the "suggested" rev to absorb to. Later,
   * the user can change this to a different rev.
   * Must be >= introductionRev.
   */
  selectedRev: FileRev | null;
  /** The "AbsorbEditId" associated with this diff chunk. */
  absorbEditId: AbsorbEditId;
  /** The file stack index (in commitState) associated with this diff chunk. */
  fileStackIndex?: FileStackIndex;
};

/**
 * Represents an absorb edit from the wdir to the stack.
 *
 * This looks like a diff chunk, with extra info like "blame" (introductionRev)
 * and "amend -to" (selectedRev). Note this is not 1:1 mapping to diff chunks,
 * since one diff chunk might be split into multiple `AbsorbEdit`s if they need
 * to be absorbed to different commits.
 */
export const AbsorbEdit = Record<AbsorbEditProps>({
  oldStart: 0,
  oldEnd: 0,
  oldLines: List(),
  newStart: 0,
  newEnd: 0,
  newLines: List(),
  introductionRev: 0 as FileRev,
  selectedRev: null,
  absorbEditId: 0,
  fileStackIndex: undefined,
});
export type AbsorbEdit = RecordOf<AbsorbEditProps>;

/**
 * Identifier of an `AbsorbEdit` in a file stack.
 */
export type AbsorbEditId = number;

/**
 * Maximum `AbsorbEditId` (exclusive). Must be an exponent of 2.
 *
 * Practically this shares the 52 bits (defined by IEEE 754) with the integer
 * part of the `Rev`.
 */
// eslint-disable-next-line no-bitwise
const MAX_ABSORB_EDIT_ID = 1 << 20;
const ABSORB_EDIT_ID_FRACTIONAL_UNIT = 1 / MAX_ABSORB_EDIT_ID;

/** Extract the "AbsorbEditId" from a linelog Rev */
export function extractRevAbsorbId(rev: FileRev): [FileRev, AbsorbEditId] {
  const fractional = rev % 1;
  const integerRev = rev - fractional;
  const absorbEditId = fractional / ABSORB_EDIT_ID_FRACTIONAL_UNIT - 1;
  assert(
    Number.isInteger(absorbEditId) && absorbEditId >= 0,
    `${rev} does not contain valid AbsorbEditId`,
  );
  return [integerRev as FileRev, absorbEditId];
}

/** Embed an absorbEditId into a Rev */
export function embedAbsorbId(rev: FileRev, absorbEditId: AbsorbEditId): FileRev {
  assert(Number.isInteger(rev), `${rev} already has an absorbEditId embedded`);
  assert(
    absorbEditId < MAX_ABSORB_EDIT_ID - 1,
    t(
      `Number of absorb diff chunks exceeds maximum limit ($count). Please retry with only a subset of the changes.`,
      {replace: {$count: (absorbEditId + 1).toString()}},
    ),
  );
  return (rev + ABSORB_EDIT_ID_FRACTIONAL_UNIT * (absorbEditId + 1)) as FileRev;
}

/**
 * Returns a rev with all absorb edits for this rev included.
 * For example, `revWithAbsorb(2)` might return something like `2.999`.
 * */
export function revWithAbsorb(rev: FileRev): FileRev {
  return (Math.floor(rev) + 1 - ABSORB_EDIT_ID_FRACTIONAL_UNIT) as FileRev;
}

/**
 * Calculate absorb edits for a stack.
 *
 * The stack top is treated as `wdir()` to be absorbed to the rest of the
 * stack. The stack bottom is treated as imutable `public()`.
 *
 * All edits in `wdir()` will be broken down and labeled with `AbsorbEditId`s.
 * If an edit with `id: AbsorbEditId` has a default absorb destination
 * `x: Rev`, then this edit will be inserted in linelog as rev
 * `embedAbsorbId(x, id)`, and can be checked out via
 * `linelog.checkOut(revWithAbsorb(x))`.
 *
 * If an edit has no default destination, for example, the surrounding lines
 * belong to public commit (rev 0), the edit will be left in the `wdir()`,
 * and can be checked out using `revWithAbsorb(wdirRev)`, where `wdirRev` is
 * the max integer rev in the linelog.
 *
 * Returns `FileStackState` with absorb edits embedded in the linelog, along
 * with a mapping from the `AbsorbEditId` to the diff chunk.
 */
export function calculateAbsorbEditsForFileStack(
  stack: FileStackState,
  options?: {fileStackIndex?: FileStackIndex},
): [FileStackState, ImMap<AbsorbEditId, AbsorbEdit>] {
  // rev 0 (public), 1, 2, ..., wdirRev-1 (stack top to absorb), wdirRev (wdir virtual rev)
  const wdirRev = prev(stack.revLength);
  assert(
    wdirRev >= 1,
    'calculateAbsorbEditsForFileStack requires at least one wdir(), one public()',
  );
  const fileStackIndex = options?.fileStackIndex;
  const diffChunks = analyseFileStackWithWdirAtTop(stack, {wdirRev, fileStackIndex});
  // Drop wdirRev, then re-insert the chunks.
  let newStack = stack.truncate(wdirRev);
  let absorbIdToDiffChunk = ImMap<AbsorbEditId, AbsorbEdit>();
  const diffChunksWithAbsorbId = diffChunks.map(chunk => {
    absorbIdToDiffChunk = absorbIdToDiffChunk.set(chunk.absorbEditId, chunk);
    return chunk;
  });
  // Re-insert the chunks with the absorbId.
  newStack = applyFileStackEditsWithAbsorbId(newStack, diffChunksWithAbsorbId);
  return [newStack, absorbIdToDiffChunk];
}

/**
 * Similar to `analyseFileStack`, but the stack contains the "wdir()" at the top:
 * The stack revisions look like: `[0:public] [1] [2] ... [stackTop] [wdir]`.
 */
export function analyseFileStackWithWdirAtTop(
  stack: FileStackState,
  options?: {wdirRev?: FileRev; fileStackIndex?: FileStackIndex},
): List<AbsorbEdit> {
  const wdirRev = options?.wdirRev ?? prev(stack.revLength);
  const stackTopRev = prev(wdirRev);
  assert(stackTopRev >= 0, 'stackTopRev must be positive');
  const newText = stack.getRev(wdirRev);
  let edits = analyseFileStack(stack, newText, stackTopRev);
  const fileStackIndex = options?.fileStackIndex;
  if (fileStackIndex != null) {
    edits = edits.map(edit => edit.set('fileStackIndex', fileStackIndex));
  }
  return edits;
}

/**
 * Given a stack and the latest changes (usually at the stack top),
 * calculate the diff chunks and the revs that they might be absorbed to.
 * The rev 0 of the file stack should come from a "public" (immutable) commit.
 */
export function analyseFileStack(
  stack: FileStackState,
  newText: string,
  stackTopRev?: FileRev,
): List<AbsorbEdit> {
  assert(stack.revLength > 0, 'stack should not be empty');
  const linelog = stack.convertToLineLog();
  const oldRev = stackTopRev ?? prev(stack.revLength);
  const oldText = stack.getRev(oldRev);
  const oldLines = splitLines(oldText);
  // The `LineInfo` contains "blame" information.
  const oldLineInfos = linelog.checkOutLines(oldRev);
  const newLines = splitLines(newText);
  const result: Array<AbsorbEdit> = [];
  let nextAbsorbId = 0;
  const allocateAbsorbId = () => {
    const id = nextAbsorbId;
    nextAbsorbId += 1;
    return id;
  };
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
    const involvedRevs = dedup(
      involvedLineInfos.map(info => info.rev as FileRev).filter(rev => rev > 0),
    );
    // Normalize `selectedRev` so it cannot be a public commit (fileRev === 0).
    // Setting to `null` to make the edit deselected (left in the working copy).
    const normalizeSelectedRev = (rev: FileRev): FileRev | null => (rev === 0 ? null : rev);
    if (involvedRevs.length === 1) {
      // Only one rev. Set selectedRev to this.
      // For simplicity, we're not checking the "continuous" lines here yet (different from Python).
      const introductionRev = involvedRevs[0];
      result.push(
        AbsorbEdit({
          oldStart: a1,
          oldEnd: a2,
          oldLines: List(oldLines.slice(a1, a2)),
          newStart: b1,
          newEnd: b2,
          newLines: List(newLines.slice(b1, b2)),
          introductionRev,
          selectedRev: normalizeSelectedRev(introductionRev),
          absorbEditId: allocateAbsorbId(),
        }),
      );
    } else if (b1 === b2) {
      // Deletion. Break the chunk into sub-chunks with different selectedRevs.
      // For simplicity, we're not checking the "continuous" lines here yet (different from Python).
      splitChunk(a1, a2, oldLineInfos, (oldStart, oldEnd, introductionRev) => {
        result.push(
          AbsorbEdit({
            oldStart,
            oldEnd,
            oldLines: List(oldLines.slice(oldStart, oldEnd)),
            newStart: b1,
            newEnd: b2,
            newLines: List([]),
            introductionRev,
            selectedRev: normalizeSelectedRev(introductionRev),
            absorbEditId: allocateAbsorbId(),
          }),
        );
      });
    } else if (a2 - a1 === b2 - b1 && involvedLineInfos.some(info => info.rev > 0)) {
      // Line count matches on both side. No public lines.
      // We assume the "a" and "b" sides are 1:1 mapped.
      // So, even if the "a"-side lines blame to different revs, we can
      // still break the chunks to individual lines.
      const delta = b1 - a1;
      splitChunk(a1, a2, oldLineInfos, (oldStart, oldEnd, introductionRev) => {
        const newStart = oldStart + delta;
        const newEnd = oldEnd + delta;
        result.push(
          AbsorbEdit({
            oldStart,
            oldEnd,
            oldLines: List(oldLines.slice(oldStart, oldEnd)),
            newStart,
            newEnd,
            newLines: List(newLines.slice(newStart, newEnd)),
            introductionRev,
            selectedRev: normalizeSelectedRev(introductionRev),
            absorbEditId: allocateAbsorbId(),
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
        AbsorbEdit({
          oldStart: a1,
          oldEnd: a2,
          oldLines: List(oldLines.slice(a1, a2)),
          newStart: b1,
          newEnd: b2,
          newLines: List(newLines.slice(b1, b2)),
          introductionRev: max(...involvedRevs, 0),
          selectedRev: null,
          absorbEditId: allocateAbsorbId(),
        }),
      );
    }
  });
  return List(result);
}

/**
 * Apply edits specified by `chunks`.
 * The `chunk.selectedRev` is expected to include the `AbsorbEditId`.
 */
export function applyFileStackEditsWithAbsorbId(
  stack: FileStackState,
  chunks: Iterable<AbsorbEdit>,
): FileStackState {
  assert(stack.revLength > 0, 'stack should not be empty');
  let linelog = stack.convertToLineLog();
  const wdirRev = stack.revLength;
  const stackTopRev = wdirRev - 1;
  // Apply the changes. Assuming there are no overlapping chunks, we apply
  // from end to start so the line numbers won't need change.
  const sortedChunks = [...chunks].toSorted((a, b) => b.oldEnd - a.oldEnd);
  sortedChunks.forEach(chunk => {
    // If not "selected" to amend to a commit, leave the chunk at the wdir.
    const baseRev = chunk.selectedRev ?? (wdirRev as FileRev);
    const absorbEditId = nullthrows(chunk.absorbEditId);
    const targetRev = embedAbsorbId(baseRev, absorbEditId);
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
      stackTopRev,
      chunk.oldStart,
      chunk.oldEnd,
      targetRev,
      chunk.newLines.toArray(),
    );
  });
  return stack.fromLineLog(linelog);
}

/** Split the start..end chunk into sub-chunks so each chunk has the same "blame" rev. */
function splitChunk(
  start: number,
  end: number,
  lineInfos: readonly LineInfo[],
  callback: (_start: number, _end: number, _introductionRev: FileRev) => void,
) {
  let lastStart = start;
  for (let i = start; i < end; i++) {
    const introductionRev = lineInfos[i].rev as FileRev;
    if (i + 1 === end || introductionRev != lineInfos[i + 1].rev) {
      callback(lastStart, i + 1, introductionRev);
      lastStart = i + 1;
    }
  }
}
