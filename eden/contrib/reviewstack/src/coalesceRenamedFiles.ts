/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitChange, Diff} from './github/diffTypes';
import type {CommitComparisonFile} from './github/restApiTypes';

import joinPath from './joinPath';

export type RenamedFile = {
  type: 'rename';
  before: Extract<CommitChange, {type: 'remove'}>;
  after: Extract<CommitChange, {type: 'add'}>;
};

export type DisplayChange = CommitChange | RenamedFile;

/**
 * Pair removed and added paths that GitHub identifies as renames. Fall back
 * to matching identical blobs when comparison metadata is unavailable.
 */
export default function coalesceRenamedFiles(
  diff: Diff,
  comparisonFiles: readonly CommitComparisonFile[] = [],
): DisplayChange[] {
  const removalsByBlob = new Map<string, number[]>();
  const additionsByBlob = new Map<string, number[]>();
  const removalsByPath = new Map<string, number>();
  const additionsByPath = new Map<string, number>();

  diff.forEach((change, index) => {
    if (change.type !== 'add' && change.type !== 'remove') {
      return;
    }
    const key = `${change.entry.oid}:${change.entry.mode}`;
    const changesByBlob = change.type === 'remove' ? removalsByBlob : additionsByBlob;
    const indexes = changesByBlob.get(key) ?? [];
    indexes.push(index);
    changesByBlob.set(key, indexes);
    const path = joinPath(change.basePath, change.entry.name);
    (change.type === 'remove' ? removalsByPath : additionsByPath).set(path, index);
  });

  const replacements = new Map<number, RenamedFile>();
  const consumed = new Set<number>();

  // GitHub can identify a rename even when the file was edited while moving.
  // Pair those paths before falling back to exact blob matches.
  comparisonFiles.forEach(file => {
    if (file.status !== 'renamed' || file.previousFilename == null) {
      return;
    }
    const removalIndex = removalsByPath.get(file.previousFilename);
    const additionIndex = additionsByPath.get(file.filename);
    if (removalIndex == null || additionIndex == null) {
      return;
    }
    const before = diff[removalIndex];
    const after = diff[additionIndex];
    if (before.type !== 'remove' || after.type !== 'add') {
      return;
    }
    replacements.set(Math.min(removalIndex, additionIndex), {type: 'rename', before, after});
    consumed.add(removalIndex);
    consumed.add(additionIndex);
  });

  removalsByBlob.forEach((removalIndexes, key) => {
    const availableRemovals = removalIndexes.filter(index => !consumed.has(index));
    const additionIndexes = (additionsByBlob.get(key) ?? []).filter(index => !consumed.has(index));
    const pairCount = Math.min(availableRemovals.length, additionIndexes.length);
    for (let pair = 0; pair < pairCount; pair++) {
      const removalIndex = availableRemovals[pair];
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
