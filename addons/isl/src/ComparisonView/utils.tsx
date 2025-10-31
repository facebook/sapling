/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {parsePatch} from 'shared/patch/parse';
import type {DiffType, ParsedDiff} from 'shared/patch/types';

export function parsePatchAndFilter(patch: string): ReturnType<typeof parsePatch> {
  const result = parsePatch(patch);
  return result.filter(
    // empty patches and other weird situations can cause invalid files to get parsed, ignore these entirely
    diff => diff.hunks.length > 0 || diff.newFileName != null || diff.oldFileName != null,
  );
}

/** Similar to how uncommitted changes are sorted, sort first by type, then by filename. */
export function sortFilesByType(files: Array<ParsedDiff>) {
  files.sort((a, b) => {
    if (a.type === b.type) {
      const pathA = a.newFileName ?? a.oldFileName ?? '';
      const pathB = b.newFileName ?? b.oldFileName ?? '';
      return pathA.localeCompare(pathB);
    } else {
      return (
        (a.type == null ? SORT_LAST : sortKeyForType[a.type]) -
        (b.type == null ? SORT_LAST : sortKeyForType[b.type])
      );
    }
  });
}
const SORT_LAST = 10;
const sortKeyForType: Record<DiffType, number> = {
  Modified: 0,
  Renamed: 1,
  Added: 2,
  Copied: 3,
  Removed: 4,
};
