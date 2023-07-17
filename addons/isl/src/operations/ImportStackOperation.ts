/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitTree} from '../getCommitTree';
import type {ApplyPreviewsFuncType, PreviewContext} from '../previews';
import type {CommitInfo} from '../types';
import type {Hash} from 'shared/types/common';
import type {ImportCommit, ImportStack, Mark} from 'shared/types/stack';

import {t} from '../i18n';
import {CommitPreview} from '../previews';
import {Operation} from './Operation';

export class ImportStackOperation extends Operation {
  static opName = 'StackEdit';

  // Derived from importStack.

  /** Commits sorted from the stack bottom to top. */
  private commits: Readonly<ImportCommit>[];

  /** Original commits that will be predecessors (being replaced). */
  private origHashes: Set<Hash>;

  /** Original commits that will be hidden. */
  private hideHashes: Set<Hash>;

  /** Parent of the first commit. */
  private firstParent: Hash | null;

  /** Goto command from the importStack. */
  private gotoMark: Mark | undefined;

  constructor(private importStack: Readonly<ImportStack>) {
    super('ImportStackOperation');

    let firstParent: Hash | null = null;
    const origHashes = new Set<Hash>();
    const hideHashes = importStack.flatMap(([op, value]) =>
      op === 'hide' ? [...value.nodes] : [],
    );
    const gotoMark = importStack
      .flatMap(([op, value]) => (op === 'goto' || op === 'reset' ? value.mark : []))
      .at(-1);
    const commits = importStack.flatMap(a => (a[0] === 'commit' ? [a[1]] : []));
    commits.forEach(commit => {
      if (firstParent == null) {
        firstParent = commit.parents.at(0) ?? 'unexpect_parents';
      }
      (commit.predecessors ?? []).forEach(pred => {
        origHashes.add(pred);
      });
    });

    this.commits = commits;
    this.firstParent = firstParent;
    this.origHashes = origHashes;
    this.hideHashes = new Set(hideHashes);
    this.gotoMark = gotoMark;
  }

  getArgs() {
    return ['debugimportstack'];
  }

  getStdin() {
    return JSON.stringify(this.importStack);
  }

  getDescriptionForDisplay() {
    return {
      description: t('Applying stack changes'),
      tooltip: t(
        'This operation does not have a traditional command line equivalent. \n' +
          'You might use commit, amend, histedit, rebase, absorb, fold, or a combination of them for similar functionalities.',
      ),
    };
  }

  makeOptimisticApplier(context: PreviewContext): ApplyPreviewsFuncType | undefined {
    // If parent is missing, then the stack is probably hidden.
    if (!this.firstParent || !context.treeMap.has(this.firstParent)) {
      return undefined;
    }

    const maybeRewriteTree = (tree: CommitTree): ReturnType<ApplyPreviewsFuncType> => {
      // `debugimportstack` runs in one transaction, new commits are all or nothing.
      // So any new commit indicates the new stack is available.
      // Note: working copy parent change is not transactional. We might
      // observe new commits before old commits disappear because the `.`
      // commit might be at the old place and keep the old commits visible.
      const haveNewStack =
        tree.children.length > 1 &&
        tree.children.some(c => c.info.closestPredecessors?.some(p => this.origHashes.has(p)));

      // Filter out the old (being edited) stack (and also YouAreHere on the old stack).
      const children = tree.children.filter(
        c => !this.origHashes.has(c.info.hash) && !this.hideHashes.has(c.info.hash),
      );

      // If the new stack is not yet ready, provide a preview tree (stack).
      if (!haveNewStack) {
        // ImportCommit[] -> CommitInfo[]
        let parents = this.firstParent ? [this.firstParent] : [];
        const previewStack: CommitInfo[] = this.commits.map(commit => {
          const pred = commit.predecessors?.at(-1);
          const existingInfo = pred ? context.treeMap.get(pred)?.info : undefined;
          // Use existing CommitInfo as the "base" to build a new CommitInfo.
          const info: CommitInfo = {
            // "Default". Might be replaced by existingInfo.
            bookmarks: [],
            remoteBookmarks: [],
            filesSample: [],
            phase: 'draft',
            hash: `fake:${commit.mark}`,
            // Note: using `existingInfo` here might be not accurate.
            ...(existingInfo || {}),
            // Replace existingInfo.
            parents,
            title: commit.text.trimStart().split('\n', 1).at(0) || '',
            author: commit.author ?? '',
            date: commit.date == null ? new Date() : new Date(commit.date[0] * 1000),
            description: commit.text,
            isHead: this.gotoMark ? commit.mark === this.gotoMark : existingInfo?.isHead ?? false,
            totalFileCount: Object.keys(commit.files).length,
          };
          parents = [info.hash];
          return info;
        });
        // CommitInfo[] -> CommitTree.
        const previewTree = previewStack
          .reverse()
          .reduce((tree: null | CommitTree, info: CommitInfo): CommitTree => {
            if (tree == null) {
              return {info, children: []};
            } else {
              return {info, children: [tree]};
            }
          }, null);
        if (previewTree != null) {
          children.push(previewTree);
        }
      }
      return {
        ...tree,
        children,
        previewType: CommitPreview.STACK_EDIT_ROOT,
        childPreviewType: CommitPreview.STACK_EDIT_DESCENDANT,
      };
    };

    const func: ApplyPreviewsFuncType = (tree, _previewType) => {
      if (tree.info.hash === this.firstParent) {
        // This tree is interesting.
        return maybeRewriteTree(tree);
      } else {
        return tree;
      }
    };

    // Hide duplicated stack
    return func;
  }
}
