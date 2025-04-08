/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepoPath} from 'shared/types/common';
import type {CommitStackState} from './commitStackState';
import type {CommitRev, FileFlag, FileRev} from './common';
import type {DiffCommit, DiffFile, DiffLine, PartiallySelectedDiffCommit} from './diffSplitTypes';

import {Set as ImSet, List, Range} from 'immutable';
import {readableDiffBlocks as diffBlocks, splitLines} from 'shared/diff';
import {nullthrows} from 'shared/utils';
import {FlattenLine} from '../linelog';
import {ABSENT_FLAG, FileState} from './common';
import {FileStackState} from './fileStackState';
import {next} from './revMath';

/** Parameters used by `diffFile()`. */
type DiffFileProps = {
  aContent: string;
  bContent: string;
  aPath: RepoPath;
  bPath: RepoPath;
  aFlag: FileFlag;
  bFlag: FileFlag;
};

/**
 * Calculate the diff for a commit. Returns a JSON-friendly format.
 * NOTE:
 * - This is not a lossless representation. Certain files (non-utf8, large) are
 *   silently ignored.
 * - Renaming x to y has 2 changes: delete x, edit y (diff against x).
 */
export function diffCommit(stack: CommitStackState, rev: CommitRev): DiffCommit {
  const commit = nullthrows(stack.get(rev));
  const aRev = commit.parents.first() ?? (-1 as CommitRev);
  const files = stack.getPaths(rev, {text: true}).flatMap(bPath => {
    const bFile = stack.getFile(rev, bPath);
    const aPath = bFile.copyFrom ?? bPath;
    const aFile = stack.getFile(aRev, aPath);
    const aContent = stack.getUtf8DataOptional(aFile);
    const bContent = stack.getUtf8DataOptional(bFile);
    if (aContent === null || bContent === null) {
      // Not utf-8.
      return [];
    }
    const aFlag = aFile.flags ?? '';
    const bFlag = bFile.flags ?? '';
    if (aContent === bContent && aFlag === bFlag) {
      // Not changed.
      return [];
    }
    return [diffFile({aContent, bContent, aPath, bPath, aFlag, bFlag})];
  });
  return {
    message: commit.text,
    files,
  };
}

/**
 * Split the `rev` into `len(selections)`. Each `newDiff` specifies a subset of
 * line changes originally from `diffCommit(stack, rev)`.
 *
 * Designed to be robust about "bad" input of `selections`:
 * - If `selections` contains line references not present in
 *   `diffCommit(stack, rev)`, they will be ignored.
 * - The last diff's line selection is ignored so we can force match
 *   the content of the original commit.
 *
 * Binary or large files that are not part of `diffCommit(stack, rev)`
 * will be moved to the last split commit.
 */
export function applyDiffSplit(
  stack: CommitStackState,
  rev: CommitRev,
  selections: ReadonlyArray<PartiallySelectedDiffCommit>,
): CommitStackState {
  const originalDiff = diffCommit(stack, rev);

  // Drop the last diff since its content is forced to match `rev`.
  const len = selections.length - 1;
  if (len < 0) {
    return stack;
  }

  // Calculate the file contents.
  const affectedFiles = new Map(originalDiff.files.map(f => [f.bPath, f]));
  const diffFiles: Array<Map<RepoPath, [Set<number>, Set<number>]>> = selections
    .slice(0, len)
    .map(d => new Map(d.files.map(f => [f.bPath, [new Set(f.aLines), new Set(f.bLines)]])));
  const allRevs = ImSet(Range(0, len));
  const noneRevs = ImSet<number>();
  const fileStacks: Map<RepoPath, FileStackState> = new Map(
    [...affectedFiles.entries()].map(([path, file]) => {
      const lines = file.lines.map(({a, b, content}) => {
        let revs = allRevs;
        if (a == null && b != null) {
          // Figure out which rev adds (selects) the line.
          const rev = diffFiles.findIndex(map => map.get(path)?.[1]?.has(b));
          revs = rev == -1 ? noneRevs : ImSet(Range(rev, len));
        } else if (b == null && a != null) {
          // Figure out which rev removes (selects) the line.
          const rev = diffFiles.findIndex(map => map.get(path)?.[0]?.has(a));
          revs = rev == -1 ? allRevs : ImSet(Range(0, rev));
        }
        return new FlattenLine({revs, data: content});
      });
      const fileStack = new FileStackState([]);
      return [path, fileStack.fromFlattenLines(List(lines), len)];
    }),
  );

  // Create new commits and populate their content.
  const copyFromMap = new Map(
    [...affectedFiles.values()].map(file => [
      file.bPath,
      file.aPath === file.bPath ? undefined : file.aPath,
    ]),
  );
  let newStack = stack;
  selections.slice(0, len).forEach((selection, i) => {
    const currentRev = next(rev, i);
    newStack = newStack.insertEmpty(currentRev, selection.message, currentRev);
    selection.files.forEach(file => {
      const content = fileStacks.get(file.bPath)?.getRev(i as FileRev);
      if (content != null) {
        // copyFrom is set when the file is first modified.
        const copyFrom: string | undefined =
          file.bFlag === ABSENT_FLAG ? undefined : copyFromMap.get(file.bPath);
        newStack = newStack.setFile(currentRev, file.bPath, _f =>
          FileState({data: content, copyFrom, flags: file.bFlag ?? ''}),
        );
        copyFromMap.delete(file.bPath);
      }
    });
  });

  // Update commit message of the last commit.
  newStack = newStack.editCommitMessage(next(rev, len), selections[len].message);

  return newStack;
}

/** Produce a readable diff for debugging or testing purpose. */
export function displayDiff(diff: DiffCommit): string {
  const output = [diff.message.trimEnd(), '\n'];
  diff.files.forEach(file => {
    output.push(`diff a/${file.aPath} b/${file.bPath}\n`);
    if (file.aFlag !== file.bFlag) {
      if (file.bFlag === ABSENT_FLAG) {
        output.push(`deleted file mode ${flagToMode(file.aFlag)}\n`);
      } else if (file.aFlag === ABSENT_FLAG) {
        output.push(`new file mode ${flagToMode(file.bFlag)}\n`);
      } else {
        output.push(`old mode ${flagToMode(file.aFlag)}\n`);
        output.push(`new mode ${flagToMode(file.bFlag)}\n`);
      }
    }
    if (file.aPath !== file.bPath) {
      output.push(`copy from ${file.aPath}\n`);
      output.push(`copy to ${file.bPath}\n`);
    }
    file.lines.forEach(line => {
      const sign = line.a == null ? '+' : line.b == null ? '-' : ' ';
      output.push(`${sign}${line.content}`);
      if (!line.content.includes('\n')) {
        output.push('\n\\ No newline at end of file');
      }
    });
  });
  return output.join('');
}

function flagToMode(flag: FileFlag): string {
  switch (flag) {
    case '':
      return '100644';
    case 'x':
      return '100755';
    case 'l':
      return '120000';
    case 'm':
      return '160000';
    default:
      return '100644';
  }
}

/** Produce `DiffFile` based on contents of both sides. */
export function diffFile({
  aContent,
  bContent,
  aPath,
  bPath,
  aFlag,
  bFlag,
}: DiffFileProps): DiffFile {
  const aLines = splitLines(aContent);
  const bLines = splitLines(bContent);
  const lines: DiffLine[] = [];
  diffBlocks(aLines, bLines).forEach(([sign, [a1, a2, b1, b2]]) => {
    if (sign === '=') {
      for (let ai = a1; ai < a2; ++ai) {
        lines.push({a: ai, b: ai + b1 - a1, content: aLines[ai]});
      }
    } else {
      for (let ai = a1; ai < a2; ++ai) {
        lines.push({a: ai, b: null, content: aLines[ai]});
      }
      for (let bi = b1; bi < b2; ++bi) {
        lines.push({a: null, b: bi, content: bLines[bi]});
      }
    }
  });
  return {
    aPath,
    bPath,
    aFlag,
    bFlag,
    lines,
  };
}
