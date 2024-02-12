/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type GitHubClient from './GitHubClient';
import type {Diff, DiffWithCommitIDs} from './diffTypes';
import type {Commit, Tree, TreeEntry} from './types';

import joinPath from '../joinPath';

/**
 * Returns null if commit does not have exactly one parent.
 */
export async function diffCommitWithParent(
  commit: Commit,
  client: GitHubClient,
): Promise<DiffWithCommitIDs | null> {
  if (commit.parents.length !== 1) {
    return null;
  }

  const parentOid = commit.parents[0];
  const parentCommit = await client.getCommit(parentOid);
  if (parentCommit == null) {
    throw new Error(`parent commit ${parentOid} could not be found`);
  }

  return diffCommits(parentCommit, commit, client);
}

export async function diffCommits(
  baseCommit: Commit,
  headCommit: Commit,
  client: GitHubClient,
): Promise<DiffWithCommitIDs> {
  const diff: Diff = [];
  await diffTree(diff, '', baseCommit.tree, headCommit.tree, client);
  return {
    diff,
    commitIDs: {
      before: baseCommit.oid,
      after: headCommit.oid,
    },
  };
}

export async function diffTree(
  diff: Diff,
  basePath: string,
  baseTree: Tree,
  headTree: Tree,
  client: GitHubClient,
): Promise<void> {
  const {entries: baseEntries} = baseTree;
  const {entries: headEntries} = headTree;
  let baseIndex = 0;
  let headIndex = 0;
  const maxBaseIndex = baseEntries.length;
  const maxHeadIndex = headEntries.length;

  /* eslint-disable no-await-in-loop */
  while (true) {
    // We define things as follows so that TypeScript thinks that baseEntry and
    // headEntry are always non-null, though that is not the case once one of
    // the lists has been exhausted.
    const hasBaseEntry = baseIndex < maxBaseIndex;
    const hasHeadEntry = headIndex < maxHeadIndex;
    const baseEntry = baseEntries[baseIndex];
    const headEntry = headEntries[headIndex];

    let compare;
    if (hasBaseEntry) {
      if (hasHeadEntry) {
        compare = compareTreeEntry(baseEntry, headEntry);
      } else {
        compare = 'less';
      }
    } else if (hasHeadEntry) {
      compare = 'greater';
    } else {
      // We have exhausted both lists.
      break;
    }

    switch (compare) {
      case 'less': {
        // baseEntry was removed in headTree
        if (baseEntry.type === 'blob') {
          diff.push({type: 'remove', basePath, entry: baseEntry});
        } else {
          const pathToSubtree = joinPath(basePath, baseEntry.name);
          await recordChangesInTree(baseEntry, pathToSubtree, 'remove', diff, client);
        }
        ++baseIndex;
        break;
      }
      case 'greater': {
        // headEntry was introduced in headTree
        if (headEntry.type === 'blob') {
          diff.push({type: 'add', basePath, entry: headEntry});
        } else {
          const pathToSubtree = joinPath(basePath, headEntry.name);
          await recordChangesInTree(headEntry, pathToSubtree, 'add', diff, client);
        }
        ++headIndex;
        break;
      }
      case 'equal': {
        ++baseIndex;
        ++headIndex;
        break;
      }
      case 'changed': {
        const isBaseBlob = baseEntry.type === 'blob';
        const isHeadBlob = headEntry.type === 'blob';
        const pathToEntry = joinPath(basePath, baseEntry.name);
        if (isBaseBlob && isHeadBlob) {
          diff.push({type: 'modify', basePath, before: baseEntry, after: headEntry});
        } else if (!isBaseBlob && !isHeadBlob) {
          const [subdirBaseTree, subdirHeadTree] = await Promise.all([
            client.getTree(baseEntry.oid),
            client.getTree(headEntry.oid),
          ]);
          if (subdirBaseTree == null) {
            throw new Error(`could not find Tree ${baseEntry.oid} for ${pathToEntry}`);
          }
          if (subdirHeadTree == null) {
            throw new Error(`could not find Tree ${headEntry.oid} for ${pathToEntry}`);
          }
          await diffTree(diff, pathToEntry, subdirBaseTree, subdirHeadTree, client);
        } else if (isBaseBlob) {
          // A blob was replaced with a tree.
          diff.push({type: 'remove', basePath, entry: baseEntry});
          await recordChangesInTree(headEntry, pathToEntry, 'add', diff, client);
        } else {
          // A tree was replaced with a blob.
          diff.push({type: 'add', basePath, entry: headEntry});
          await recordChangesInTree(baseEntry, pathToEntry, 'remove', diff, client);
        }
        ++baseIndex;
        ++headIndex;
        break;
      }
    }
  }
  /* eslint-enable no-await-in-loop */
}

async function recordChangesInTree(
  treeEntry: TreeEntry,
  pathToTreeEntry: string,
  type: 'add' | 'remove',
  diff: Diff,
  client: GitHubClient,
): Promise<void> {
  const tree = await client.getTree(treeEntry.oid);
  if (tree == null) {
    return;
  }

  for (const entry of tree.entries) {
    if (entry.type === 'blob') {
      diff.push({
        type,
        basePath: pathToTreeEntry,
        entry,
      });
    } else {
      const pathToSubtree = joinPath(pathToTreeEntry, entry.name);
      // Although we may want to consider using Promise.all() to do more
      // fetching in parallel, for now, we implement things this way to ensure
      // changes are recorded in depth-first pre-order.
      // eslint-disable-next-line no-await-in-loop
      await recordChangesInTree(entry, pathToSubtree, type, diff, client);
    }
  }
}

type TreeEntryCompare = 'less' | 'greater' | 'equal' | 'changed';

export function compareTreeEntry(first: TreeEntry, second: TreeEntry): TreeEntryCompare {
  const {name: name1} = first;
  const {name: name2} = second;
  if (name1 < name2) {
    return 'less';
  } else if (name1 > name2) {
    return 'greater';
  }

  const {oid: oid1, mode: mode1, type: type1} = first;
  const {oid: oid2, mode: mode2, type: type2} = second;
  return oid1 === oid2 && mode1 === mode2 && type1 === type2 ? 'equal' : 'changed';
}
