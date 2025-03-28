/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepoPath} from 'shared/types/common';
import type {CommitStackState} from './commitStackState';
import type {ABSENT_FLAG, CommitRev, FileFlag} from './common';

import {diffBlocks, splitLines} from 'shared/diff';
import {nullthrows} from 'shared/utils';

/***
 * Commit diff in a JSON-friendly format.
 * Lossly. Might not include all changes in the commit (ex. large or binary
 * files).
 * Initially intended as input to a remote "split" service.
 */
export type DiffCommit = {
  /*** Commit message. */
  message: string;
  /*** Diff-able (text and non-large) files. */
  files: ReadonlyArray<DiffFile>;
};

/** Unified diff represent in a JSON-friendly format. */
export type DiffFile = {
  /** File path on the left side (previous version). */
  aPath: RepoPath;
  /** File path on the right side (current version). */
  bPath: RepoPath;
  /**
   * File flag on the left side (previous version).
   * '': normal; 'x': executable; 'l': symlink; 'a': absent (deleted); 'm': submodule.
   * Cannot be ".".
   */
  aFlag: FileFlag;
  /** File flag on the right side (current version). */
  bFlag: FileFlag;
  /** Unified diff. See `DiffLine`. */
  lines: ReadonlyArray<DiffLine>;
};

/** A line in unified diff. */
export type DiffLine = {
  /**
   * Line number on the left side (previous version).
   * Starting from 0.
   * `null` means the line does not exist on the left side,
   * aka. the line was added.
   */
  a: number | null;
  /**
   * Line number on the right side (current version).
   * Starting from 0.
   * `null` means the line does not exist on the right side,
   * aka. the line was deleted.
   */
  b: number | null;
  /**
   * Line content.
   * Trailing new-line is preserved.
   * The last line might have no trailing new-line.
   */
  content: string;
};

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
