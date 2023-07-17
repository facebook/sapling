/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from 'isl/src/types';

import {diffLines} from 'diff';

export function getRealignedBlameInfo(
  baseBlame: Array<[line: string, info: CommitInfo | undefined]>,
  newCode: string,
): Array<[line: string, info: CommitInfo | undefined]> {
  // TODO: we could refuse to realign for gigantic files, since this is done synchronously it could affect perf.

  const baseCode = baseBlame.map(l => l[0]).join('');

  const lineDiffs = diffLines(baseCode, newCode);

  const newRevisionInfo = new Array<[line: string, info: CommitInfo | undefined]>();
  let oldPos = 0;
  for (const change of lineDiffs) {
    const count = change.count ?? 1;
    if (change.added) {
      for (let i = 0; i < count; i++) {
        newRevisionInfo.push(['', undefined]);
      }
    } else if (change.removed) {
      oldPos += count;
    } else {
      for (let i = 0; i < count; i++) {
        newRevisionInfo.push(baseBlame[oldPos]);
        oldPos += 1;
      }
    }
  }
  return newRevisionInfo;
}
