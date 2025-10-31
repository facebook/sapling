/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hunk, ParsedDiff} from './types';
import {DiffType} from './types';

/**
 * Convert a parsed diff back to git diff format string.
 *
 * This is the reverse operation of parsePatch in parse.ts.
 */
export function stringifyPatch(parsedDiffs: ParsedDiff[]): string {
  return parsedDiffs.map(stringifyDiff).join('');
}

function stringifyDiff(diff: ParsedDiff): string {
  const parts: string[] = [];

  // diff header
  if (diff.oldFileName && diff.newFileName) {
    parts.push(`diff --git ${diff.oldFileName} ${diff.newFileName}\n`);
  }

  // extended header lines
  if (diff.type === DiffType.Renamed) {
    const oldName = diff.oldFileName?.replace(/^a\//, '') ?? '';
    const newName = diff.newFileName?.replace(/^b\//, '') ?? '';
    parts.push(`rename from ${oldName}\n`);
    parts.push(`rename to ${newName}\n`);
  }

  if (diff.type === DiffType.Copied) {
    const oldName = diff.oldFileName?.replace(/^a\//, '') ?? '';
    const newName = diff.newFileName?.replace(/^b\//, '') ?? '';
    parts.push(`copy from ${oldName}\n`);
    parts.push(`copy to ${newName}\n`);
  }

  if (diff.oldMode && diff.newMode && diff.oldMode !== diff.newMode) {
    parts.push(`old mode ${diff.oldMode}\n`);
    parts.push(`new mode ${diff.newMode}\n`);
  }

  if (diff.type === DiffType.Added && diff.newMode) {
    parts.push(`new file mode ${diff.newMode}\n`);
  }

  if (diff.type === DiffType.Removed && diff.newMode) {
    parts.push(`deleted file mode ${diff.newMode}\n`);
  }

  // file headers
  if (diff.hunks.length > 0) {
    const oldFile = diff.type === DiffType.Added ? '/dev/null' : (diff.oldFileName ?? '/dev/null');
    const newFile =
      diff.type === DiffType.Removed ? '/dev/null' : (diff.newFileName ?? '/dev/null');
    parts.push(`--- ${oldFile}\n`);
    parts.push(`+++ ${newFile}\n`);
  }

  // hunks
  diff.hunks.forEach(hunk => {
    parts.push(stringifyHunk(hunk));
  });

  return parts.join('');
}

function stringifyHunk(hunk: Hunk): string {
  const parts: string[] = [];

  // Handle the Unified Diff Format quirk:
  // If the hunk size is 0, the start line is one higher than stored
  let oldStart = hunk.oldStart;
  let newStart = hunk.newStart;

  if (hunk.oldLines === 0) {
    oldStart -= 1;
  }
  if (hunk.newLines === 0) {
    newStart -= 1;
  }

  // hunk header - always include line count
  const oldRange = `${oldStart},${hunk.oldLines}`;
  const newRange = `${newStart},${hunk.newLines}`;
  parts.push(`@@ -${oldRange} +${newRange} @@\n`);

  // hunk lines
  hunk.lines.forEach((line, index) => {
    const delimiter = hunk.linedelimiters[index] ?? '\n';
    parts.push(line + delimiter);
  });

  return parts.join('');
}
