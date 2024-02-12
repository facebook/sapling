/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {AddChange, CommitChange, Diff, ModifyChange, RemoveChange} from './diffTypes';

import {getPathForChange} from '../utils';
import {compareTreeEntry} from './diff';

/**
 * The expectation is that each `Diff` is the result of diffing the commit
 * with its base parent.
 */
export function diffVersions(beforeDiff: Diff, afterDiff: Diff): Diff {
  if (!isStrictlyIncreasing(beforeDiff) || !isStrictlyIncreasing(afterDiff)) {
    throw new Error('diffs are not sorted');
  }

  const diff: Diff = [];
  let beforeIndex = 0;
  let afterIndex = 0;
  const maxBeforeIndex = beforeDiff.length;
  const maxAfterIndex = afterDiff.length;

  while (true) {
    const hasBeforeEntry = beforeIndex < maxBeforeIndex;
    const hasAfterEntry = afterIndex < maxAfterIndex;
    const beforeChange = beforeDiff[beforeIndex];
    const afterChange = afterDiff[afterIndex];

    let pathCompare;
    if (hasBeforeEntry) {
      if (hasAfterEntry) {
        pathCompare = depthFirstPathCompare(
          getPathForChange(beforeChange),
          getPathForChange(afterChange),
        );
      } else {
        pathCompare = 'less';
      }
    } else if (hasAfterEntry) {
      pathCompare = 'greater';
    } else {
      // We have exhausted both lists.
      break;
    }

    switch (pathCompare) {
      case 'less': {
        // If change exists only in "before", then it was reversed in "after"
        diff.push(createInverse(beforeChange));
        ++beforeIndex;
        break;
      }
      case 'greater': {
        // TODO: More accurately handle all cases where change exists only in
        // "after". Returning the change as is may work only when the file
        // exists in "before" and was not modified through a rebase.
        diff.push(afterChange);
        ++afterIndex;
        break;
      }
      case 'equal': {
        // Both changes touched the same file.
        // If "before" is 'add', then "after" can only be 'add'.
        // If "before" is 'remove', then "after" can only be 'remove' or 'modify'.
        // If "before" is 'modify', then "after" can only be 'remove' or 'modify'.
        switch (beforeChange.type) {
          case 'add': {
            const change = computeChangeFromAdd(beforeChange, afterChange);
            if (change != null) {
              diff.push(change);
            }
            break;
          }
          case 'remove': {
            const change = computeChangeFromRemove(beforeChange, afterChange);
            if (change != null) {
              diff.push(change);
            }
            break;
          }
          case 'modify': {
            const change = computeChangeFromModify(beforeChange, afterChange);
            if (change != null) {
              diff.push(change);
            }
            break;
          }
        }
        ++beforeIndex;
        ++afterIndex;
        break;
      }
    }
  }

  return diff;
}

/**
 * Compute the CommitChange for a file between two versions of a commit where
 * the file was marked as an 'add' in the earlier version.
 */
function computeChangeFromAdd(v1: AddChange, v2: CommitChange): CommitChange | null {
  switch (v2.type) {
    case 'add': {
      // If the file was marked as 'add' in both V1 and V2, then the change
      // between the two, if one exists, should appear as a modification.
      if (compareTreeEntry(v1.entry, v2.entry) !== 'equal') {
        return {
          type: 'modify',
          basePath: v1.basePath,
          before: v1.entry,
          after: v2.entry,
        };
      } else {
        return null;
      }
    }
    case 'modify':
      // If the change was marked as an 'add' in V1, but now appears as a
      // 'modify' in V2, then V2 must have been rebased on a new commit where
      // the file already exists.
      return {
        type: 'modify',
        basePath: v1.basePath,
        before: v1.entry,
        after: v2.after,
      };
    case 'remove':
      // If the change was marked as an 'add' in V1, but now appears as a
      // 'remove' in V2, then V2 must have been rebased on a new commit where
      // the file already exists.
      return v2;
  }
}

/**
 * Compute the CommitChange for a file between two versions of a commit where
 * the file was marked as a 'remove' in the earlier version.
 */
function computeChangeFromRemove(v1: RemoveChange, v2: CommitChange): CommitChange | null {
  switch (v2.type) {
    case 'add':
      // If the change was marked as a 'remove' in V1, but now appears as an
      // 'add' in V2, then V2 must have been rebased on a new commit where
      // the file did not exist.
      return v2;
    case 'modify':
      // If the change was marked as a 'remove' in V1, but now appears as a
      // 'modify' in V2, then we communicate this as a modification.
      return {
        type: 'modify',
        basePath: v1.basePath,
        // Using v1.entry does not accurately describe the "before" state in
        // that it did not exist at all in v1. We need to expand the return
        // type to communicate this edge case.
        before: v1.entry,
        after: v2.after,
      };
    case 'remove': {
      if (compareTreeEntry(v1.entry, v2.entry) !== 'equal') {
        // This is an interesting case where the file was removed in both
        // versions, but the *version* of the file that was removed changed
        // between versions (V2 was likely rebased on a change to the file).
        // We return `null` because the net result is the same, though it might
        // be worth including some metadata to communicate this edge case.
        return null;
      } else {
        return null;
      }
    }
  }
}

function computeChangeFromModify(v1: ModifyChange, v2: CommitChange): CommitChange | null {
  switch (v2.type) {
    case 'add':
      // If the change was marked as a 'modify' in V1, but now appears as an
      // 'add' in V2, then V2 must have been rebased on a new commit where
      // the file did not exist.
      return v2;
    case 'modify': {
      const before = v1.after;
      const after = v2.after;
      if (compareTreeEntry(before, after) !== 'equal') {
        return {
          type: 'modify',
          basePath: v1.basePath,
          before,
          after,
        };
      } else {
        return null;
      }
    }
    case 'remove':
      // If the change was marked as a 'modify' in V1, but now appears as a
      // 'remove' in V2, then we communicate this as a removal.
      return v2;
  }
}

function createInverse(change: CommitChange): CommitChange {
  switch (change.type) {
    case 'add': {
      const {basePath, entry} = change;
      return {type: 'remove', basePath, entry};
    }
    case 'remove': {
      const {basePath, entry} = change;
      return {type: 'add', basePath, entry};
    }
    case 'modify': {
      const {basePath, before, after} = change;
      return {type: 'modify', basePath, before: after, after: before};
    }
  }
}

type Ordering = 'less' | 'equal' | 'greater';

export function depthFirstPathCompare(a: string, b: string): Ordering {
  const [aFirst, aRest] = splitOffFirstPathComponent(a);
  const [bFirst, bRest] = splitOffFirstPathComponent(b);
  if (aFirst === bFirst) {
    if (aFirst !== '') {
      return depthFirstPathCompare(aRest, bRest);
    } else {
      return stringCompare(aRest, bRest);
    }
  } else {
    return stringCompare(aFirst, bFirst);
  }
}

function stringCompare(a: string, b: string): Ordering {
  if (a < b) {
    return 'less';
  } else if (a === b) {
    return 'equal';
  } else {
    return 'greater';
  }
}

/**
 * In order to facilitate a depth-first ordering, always consider the
 * lexicographical value of the current path component instead of giving
 * priority to leaves.
 */
export function splitOffFirstPathComponent(path: string): [string, string] {
  const index = path.indexOf('/');
  if (index === -1) {
    return [path, ''];
  } else {
    return [path.slice(0, index), path.slice(index + 1)];
  }
}

export function isStrictlyIncreasing(diff: Diff): boolean {
  return diff.every((commit, index) => {
    if (index === 0) {
      return true;
    }
    const prev = diff[index - 1];
    const ordering = depthFirstPathCompare(getPathForChange(prev), getPathForChange(commit));
    return ordering === 'less';
  });
}
