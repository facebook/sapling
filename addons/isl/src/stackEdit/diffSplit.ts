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
import type {Repository} from 'isl-server/src/Repository';
import type {RepositoryContext} from 'isl-server/src/serverTypes';
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
/**
 * Calculate the diff between two commits using `sl debugexport stack`.
 * This is similar to `diffCommit` but works with commit hashes instead of CommitStackState.
 *
 * @param runSlCommand - Function to run sl commands, typically from SaplingRepository.runSlCommand
 * @param commitHash - The commit hash to diff
 * @param parentHash - The parent commit hash to diff against
 * @returns DiffCommit containing the message and file diffs
 */
export async function diffCurrentCommit(
  repo: Repository,
  ctx: RepositoryContext,
): Promise<DiffCommit> {
  // Export both commits
  const results = await repo.runCommand(
    ['debugexportstack', '-r', '.|.^'],
    'ExportStackCommand',
    ctx,
  );

  if (results.exitCode !== 0) {
    throw new Error(`Failed to export commit . ${results.stderr}`);
  }

  // Parse the exported stacks
  const stack: Array<{
    node: string;
    text: string;
    requested: boolean;
    files?: {[path: string]: {data?: string; flags?: FileFlag; copyFrom?: RepoPath} | null};
    relevantFiles?: {[path: string]: {data?: string; flags?: FileFlag; copyFrom?: RepoPath} | null};
  }> = JSON.parse(results.stdout);
  const requestedCommits = stack.filter(commit => commit.requested);

  if (requestedCommits.length !== 2) {
    throw new Error(`Expected 2 commits from debugexportstack, got ${requestedCommits.length}`);
  }

  // The second requested commit is the current one (.), the first is parent (.^)
  // because debugexportstack sorts topologically (ancestors first, descendants last)
  const parentCommit = requestedCommits[0];
  const currentCommit = requestedCommits[1];

  // Get all file paths from the commit
  const commitFiles = currentCommit.files ?? {};
  const parentFiles = parentCommit.files ?? {};
  const parentRelevantFiles = parentCommit.relevantFiles ?? {};

  // Collect all paths that changed
  const allPaths = new Set([...Object.keys(commitFiles)]);

  const files = [];
  for (const bPath of allPaths) {
    const bFile = commitFiles[bPath];
    const aPath = bFile?.copyFrom ?? bPath;
    // Get parent file from either files or relevantFiles
    const aFile =
      aPath === bPath
        ? (parentFiles[bPath] ?? parentRelevantFiles[bPath])
        : (parentFiles[aPath] ?? parentRelevantFiles[aPath]);

    const aContent = aFile?.data ?? '';
    const bContent = bFile?.data ?? '';

    // Skip if both are null (shouldn't happen, but be safe)
    if (aFile === null && bFile === null) {
      continue;
    }

    const aFlag = aFile?.flags ?? '';
    const bFlag = bFile?.flags ?? '';

    // Skip if content and flags are unchanged
    if (aContent === bContent && aFlag === bFlag) {
      continue;
    }

    const diff = diffFile({aContent, bContent, aPath, bPath, aFlag, bFlag});
    const reducedLines = reduceContextualLines(diff.lines, 10);
    files.push({...diff, lines: reducedLines});
  }

  return {
    message: currentCommit.text,
    files,
  };
}

export type PhabricatorAiDiffSplitCommitDiffFileLine = {
  a: number | null;
  b: number | null;
  content: string;
};

/**
 * Reduces the number of lines in a diff by keeping only the lines that are within
 * a specified number of lines from a changed line.
 *
 * @param lines The lines to filter
 * @param maxContextLines The maximum number of lines to keep around each changed line
 * @returns A new array with only the lines that are within the specified number of lines from a changed line
 */
export function reduceContextualLines(
  lines: ReadonlyArray<PhabricatorAiDiffSplitCommitDiffFileLine>,
  maxContextLines: number = 3,
): Array<PhabricatorAiDiffSplitCommitDiffFileLine> {
  const distanceToLastClosestChangedLine: number[] = [];
  let lastClosestChangedLineIndex = -1;

  for (let lineIndex = 0; lineIndex < lines.length; lineIndex++) {
    const line = lines[lineIndex];

    const a = line.a;
    const b = line.b;
    if ((a == null && b != null) || (a != null && b == null)) {
      // line was added or removed
      lastClosestChangedLineIndex = lineIndex;
    }

    if (lastClosestChangedLineIndex === -1) {
      distanceToLastClosestChangedLine.push(Number.MAX_SAFE_INTEGER);
    } else {
      distanceToLastClosestChangedLine.push(lineIndex - lastClosestChangedLineIndex);
    }
  }

  const distanceToNextClosestChangedLine: number[] = [];
  let nextClosestChangedLineIndex = -1;

  for (let lineIndex = lines.length - 1; lineIndex >= 0; lineIndex--) {
    const line = lines[lineIndex];

    const a = line.a;
    const b = line.b;
    if ((a == null && b != null) || (a != null && b == null)) {
      // line was added or removed
      nextClosestChangedLineIndex = lineIndex;
    }

    if (nextClosestChangedLineIndex === -1) {
      distanceToNextClosestChangedLine.push(Number.MAX_SAFE_INTEGER);
    } else {
      distanceToNextClosestChangedLine.push(nextClosestChangedLineIndex - lineIndex);
    }
  }

  // Reverse the array since we built it backwards
  distanceToNextClosestChangedLine.reverse();

  const newLines: Array<PhabricatorAiDiffSplitCommitDiffFileLine> = [];

  for (let lineIndex = 0; lineIndex < lines.length; lineIndex++) {
    if (
      distanceToLastClosestChangedLine[lineIndex] <= maxContextLines ||
      distanceToNextClosestChangedLine[lineIndex] <= maxContextLines
    ) {
      newLines.push(lines[lineIndex]);
    }
  }

  return newLines;
}
