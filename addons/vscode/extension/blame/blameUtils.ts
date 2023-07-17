/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from 'isl/src/types';

import {diffLines, splitLines} from 'shared/diff';

export function getRealignedBlameInfo(
  baseBlame: Array<[line: string, info: CommitInfo | undefined]>,
  newCode: string,
): Array<[line: string, info: CommitInfo | undefined]> {
  // TODO: we could refuse to realign for gigantic files, since this is done synchronously it could affect perf.

  const baseLines = baseBlame.map(l => l[0]);
  const newLines = splitLines(newCode);

  const lineDiffs = diffLines(baseLines, newLines);

  const newRevisionInfo = [...baseBlame];
  let accumulatedOffset = 0;

  // apply each change to the list of blame
  for (const [a1, a2, b1, b2] of lineDiffs) {
    const newEntries = new Array<[string, CommitInfo | undefined]>(b2 - b1).fill(['', undefined]);

    newRevisionInfo.splice(a1 + accumulatedOffset, a2 - a1, ...newEntries);

    // We removed (a2-a1) entries, then added (b2-b1) entries,
    // which means the *next* a1 index that previously pointed in baseBlame
    // needs to be offset according to this change since we're modifying newRevisionInfo in-place.
    accumulatedOffset += b2 - b1 - (a2 - a1);
  }

  return newRevisionInfo;
}
