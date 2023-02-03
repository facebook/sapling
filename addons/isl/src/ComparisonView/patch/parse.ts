/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export interface Hunk {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  lines: string[];
  linedelimiters: string[];
}

export enum DiffType {
  Modified,
  Added,
  Removed,
  Renamed,
  Copied,
}

export interface ParsedDiff {
  type: DiffType;
  index?: string;
  oldFileName?: string;
  newFileName?: string;
  oldHeader?: string;
  newHeader?: string;
  oldMode?: string;
  newMode?: string;
  hunks: Hunk[];
}

/**
 * Parse git/unified diff format string.
 *
 * The diff library we were using does not support git diff format (rename,
 * copy, empty file, file mode change etc). This function is to extend the
 * original `parsePatch` function [1] and make it support git diff format [2].
 *
 * [1] https://github.com/DefinitelyTyped/DefinitelyTyped/blob/master/types/diff/index.d.ts#L388
 * [2] https://github.com/git/git-scm.com/blob/main/spec/data/diff-generate-patch.txt
 */
export function parsePatch(_diffStr: string): ParsedDiff[] {
  const list: ParsedDiff[] = [];
  return list;
}
