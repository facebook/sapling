/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitPreview} from './previews';
import type {CommitInfo} from './types';

export type CommitTree = {
  info: CommitInfo;
  children: Array<CommitTree>;
};

export type CommitTreeWithPreviews = {
  info: CommitInfo;
  children: Array<CommitTreeWithPreviews>;
  previewType?: CommitPreview;
};

const byTimeDecreasing = (a: CommitInfo, b: CommitInfo) => b.date.getTime() - a.date.getTime();

/**
 * Given a list of commits from disk, produce a tree capturing the
 * parent/child structure of the commits.
 *  - Public commits are always top level (on the main line)
 *  - Public commits are sorted by date
 *  - Draft commits are always offshoots of public commits (never on main line)
 *     - Caveat: if there are no public commits found, use the parent of everything
 *       as if it were a public commit
 *  - If a public commit has no draft children, it is hidden
 *     - ...unless it has a bookmark
 *  - If a commit has multiple children, they are sorted by date
 */
export function getCommitTree(commits: Array<CommitInfo>): Array<CommitTree> {
  const childNodesByParent = new Map<string, Set<CommitInfo>>();
  commits.forEach(commit => {
    const [parent] = commit.parents;
    if (!parent) {
      return;
    }
    let set = childNodesByParent.get(parent);
    if (!set) {
      set = new Set();
      childNodesByParent.set(parent, set);
    }
    set.add(commit);
  });

  const makeTree = (revision: CommitInfo): CommitTree => {
    const {hash} = revision;
    const childrenSet = childNodesByParent.get(hash) ?? [];

    const childrenInfos = [...childrenSet].sort(byTimeDecreasing);

    const children: Array<CommitTree> =
      childrenInfos == null
        ? []
        : // only make branches off the main line for non-public revisions
          childrenInfos.filter(child => child.phase !== 'public').map(makeTree);

    return {
      info: revision,
      children,
    };
  };

  const initialCommits = commits.filter(
    commit => commit.phase === 'public' || commit.parents.length === 0,
  );

  // build tree starting from public revisions
  return initialCommits.sort(byTimeDecreasing).map(makeTree);
}

export function* walkTreePostorder(commitTree: Array<CommitTree>): IterableIterator<CommitTree> {
  for (const node of commitTree) {
    if (node.children.length > 0) {
      yield* walkTreePostorder(node.children);
    }
    yield node;
  }
}

export function isDescendant(hash: string, commitTree: CommitTree): boolean {
  for (const commit of walkTreePostorder([commitTree])) {
    if (commit.info.hash === hash) {
      return true;
    }
  }
  return false;
}
