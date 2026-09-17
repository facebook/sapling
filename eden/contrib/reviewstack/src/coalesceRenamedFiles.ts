/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitChange, Diff} from './github/diffTypes';

export type RenamedFile = {
  type: 'rename';
  before: Extract<CommitChange, {type: 'remove'}>;
  after: Extract<CommitChange, {type: 'add'}>;
};

export type DisplayChange = CommitChange | RenamedFile;

/**
 * Git stores a content-preserving rename as one removed path and one added
 * path that point at the same blob. Pair those entries so the UI can show one
 * compact rename row without fetching either copy of the file.
 */
export default function coalesceRenamedFiles(diff: Diff): DisplayChange[] {
  const removalsByBlob = new Map<string, number[]>();
  const additionsByBlob = new Map<string, number[]>();

  diff.forEach((change, index) => {
    if (change.type !== 'add' && change.type !== 'remove') {
      return;
    }
    const key = `${change.entry.oid}:${change.entry.mode}`;
    const changesByBlob = change.type === 'remove' ? removalsByBlob : additionsByBlob;
    const indexes = changesByBlob.get(key) ?? [];
    indexes.push(index);
    changesByBlob.set(key, indexes);
  });

  const replacements = new Map<number, RenamedFile>();
  const consumed = new Set<number>();
  removalsByBlob.forEach((removalIndexes, key) => {
    const additionIndexes = additionsByBlob.get(key) ?? [];
    const pairCount = Math.min(removalIndexes.length, additionIndexes.length);
    for (let pair = 0; pair < pairCount; pair++) {
      const removalIndex = removalIndexes[pair];
      const additionIndex = additionIndexes[pair];
      const before = diff[removalIndex];
      const after = diff[additionIndex];
      if (before.type !== 'remove' || after.type !== 'add') {
        continue;
      }
      const displayIndex = Math.min(removalIndex, additionIndex);
      replacements.set(displayIndex, {type: 'rename', before, after});
      consumed.add(removalIndex);
      consumed.add(additionIndex);
    }
  });

  return diff.reduce<DisplayChange[]>((displayChanges, change, index) => {
    const replacement = replacements.get(index);
    if (replacement != null) {
      displayChanges.push(replacement);
    } else if (!consumed.has(index)) {
      displayChanges.push(change);
    }
    return displayChanges;
  }, []);
}
