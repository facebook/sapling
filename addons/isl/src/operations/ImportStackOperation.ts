/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Dag, WithPreviewType} from '../previews';
import type {CommitInfo} from '../types';
import type {Hash} from 'shared/types/common';
import type {ExportStack, ImportCommit, ImportStack, Mark} from 'shared/types/stack';

import {HashSet} from '../dag/set';
import {t} from '../i18n';
import {CommitPreview} from '../previews';
import {Operation} from './Operation';

export class ImportStackOperation extends Operation {
  static opName = 'StackEdit';

  // Derived from importStack.

  /** Commits sorted from the stack bottom to top. */
  private commits: Readonly<ImportCommit>[];

  /** Parent of the first commit. */
  private firstParent: Hash | null;

  /** Goto command from the importStack. */
  private gotoMark: Mark | undefined;

  constructor(
    private importStack: Readonly<ImportStack>,
    protected originalStack: Readonly<ExportStack>,
  ) {
    super('ImportStackOperation');

    let firstParent: Hash | null = null;
    const origHashes = new Set<Hash>();
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

  optimisticDag(dag: Dag): Dag {
    const originalHashes = this.originalStack.map(c => c.node);
    // Replace the old stack with the new stack, followed by a rebase.
    // Note the rebase is actually not what the operation does, but we always
    // follow up with a rebase opeation if needed.
    const toRebase = dag.descendants(dag.children(originalHashes.at(-1)));
    let toRemove = HashSet.fromHashes(originalHashes).subtract(dag.ancestors(this.firstParent));
    // If the "toRemove" part of the original stack is gone, consider as completed.
    // Note: We no longer do a rebase in this case, and requires the rebase preview
    // to be handled separately.
    if (dag.present(toRemove).size === 0) {
      return dag;
    }
    // It's possible that the new stack was actually created but the head commit
    // keeps the old stack from disappearing (so the above check returns false).
    // In this case, we hide the new stack (by using successors) temporarily.
    // Otherwise we need to figure out the "new head", which is not trivial.
    toRemove = toRemove.union(dag.successors(toRemove));
    const newStack = this.previewStack(dag);
    const newDag = dag.remove(toRemove).add(newStack).rebase(toRebase, newStack.at(-1)?.hash);
    return newDag;
  }

  private previewStack(dag: Dag): Array<CommitInfo & WithPreviewType> {
    let parents = this.firstParent ? [this.firstParent] : [];
    const usedHashes = new Set<Hash>();
    return this.commits.map(commit => {
      const pred = commit.predecessors?.at(-1);
      const existingInfo = pred ? dag.get(pred) : undefined;
      // Pick a unique hash.
      let hash = existingInfo?.hash ?? `fake:${commit.mark}`;
      while (usedHashes.has(hash)) {
        hash = hash + '_';
      }
      usedHashes.add(hash);
      // Use existing CommitInfo as the "base" to build a new CommitInfo.
      const info: CommitInfo & WithPreviewType = {
        // "Default". Might be replaced by existingInfo.
        bookmarks: [],
        remoteBookmarks: [],
        filesSample: [],
        phase: 'draft',
        // Note: using `existingInfo` here might be not accurate.
        ...(existingInfo || {}),
        // Replace existingInfo.
        hash,
        parents,
        title: commit.text.trimStart().split('\n', 1).at(0) || '',
        author: commit.author ?? '',
        date: commit.date == null ? new Date() : new Date(commit.date[0] * 1000),
        description: commit.text,
        isHead: this.gotoMark ? commit.mark === this.gotoMark : existingInfo?.isHead ?? false,
        totalFileCount: Object.keys(commit.files).length,
        closestPredecessors: commit.predecessors,
        previewType: CommitPreview.STACK_EDIT_DESCENDANT,
      };
      parents = [info.hash];
      return info;
    });
  }
}
