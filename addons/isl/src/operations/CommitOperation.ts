/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PartialSelection} from '../partialSelection';
import type {
  ApplyUncommittedChangesPreviewsFuncType,
  Dag,
  UncommittedChangesPreviewContext,
} from '../previews';
import type {
  ChangedFile,
  CommandArg,
  CommitInfo,
  Hash,
  RepoRelativePath,
  UncommittedChanges,
} from '../types';
import type {ImportStack} from 'shared/types/stack';

import {globalRecoil} from '../AccessGlobalRecoil';
import {t} from '../i18n';
import {uncommittedChangesWithPreviews} from '../previews';
import {Operation} from './Operation';

export class CommitOperation extends Operation {
  private beforeCommitDate: Date;

  /**
   * @param message the commit message. The first line is used as the title.
   * @param originalHeadHash the hash of the current head commit, needed to track when optimistic state is resolved.
   * @param filesPathsToCommit if provided, only these file paths will be included in the commit operation. If undefined, ALL uncommitted changes are included. Paths should be relative to repo root.
   */
  constructor(
    private message: string,
    private originalHeadHash: Hash,
    private filesPathsToCommit?: Array<RepoRelativePath>,
  ) {
    super('CommitOperation');

    // New commit should have a greater date.
    this.beforeCommitDate = new Date();

    // When rendering optimistic state, we need to know the set of files that will be part of this commit.
    // This is not necessarily the same as filePathsToCommit, since it may be undefined to represent "all files".
    // This is done once at Operation creation time, not on each call to optimisticDag, since we
    // only care about the list of changed files when the CommitOperation was enqueued.
    this.optimisticChangedFiles = (
      globalRecoil().getLoadable(uncommittedChangesWithPreviews).valueMaybe() ?? []
    ).filter(changedFile => {
      return filesPathsToCommit == null
        ? true
        : filesPathsToCommit.some(f => f === changedFile.path);
    });
  }

  private optimisticChangedFiles: Array<ChangedFile>;

  static opName = 'Commit';

  getArgs() {
    const args: Array<CommandArg> = ['commit', '--addremove', '--message', this.message];
    if (this.filesPathsToCommit) {
      args.push(
        ...this.filesPathsToCommit.map(file =>
          // tag file arguments specialy so the remote repo can convert them to the proper cwd-relative format.
          ({
            type: 'repo-relative-file' as const,
            path: file,
          }),
        ),
      );
    }
    return args;
  }

  makeOptimisticUncommittedChangesApplier?(
    context: UncommittedChangesPreviewContext,
  ): ApplyUncommittedChangesPreviewsFuncType | undefined {
    const filesToCommit = new Set(this.filesPathsToCommit);
    // optimistic state is over when there's no uncommitted changes that we wanted to commit left
    if (
      context.uncommittedChanges.length === 0 ||
      (filesToCommit.size > 0 &&
        context.uncommittedChanges.every(change => !filesToCommit.has(change.path)))
    ) {
      return undefined;
    }

    const func: ApplyUncommittedChangesPreviewsFuncType = (changes: UncommittedChanges) => {
      if (this.filesPathsToCommit != null) {
        return changes.filter(change => !filesToCommit.has(change.path));
      } else {
        return [];
      }
    };
    return func;
  }

  optimisticDag(dag: Dag): Dag {
    const base = this.originalHeadHash;
    const baseInfo = dag.get(base);
    if (!baseInfo) {
      return dag;
    }

    const [title] = this.message.split(/\n+/, 1);
    const children = dag.children(base);
    const hasWantedChild = children.toHashes().some(h => {
      const info = dag.get(h);
      return info?.title === title && info?.date > this.beforeCommitDate;
    });
    if (hasWantedChild) {
      // A new commit was made on `base` with the the expected title.
      // Consider the commit operation as completed.
      return dag;
    }

    // NOTE: We might want to check the "active bookmark" state
    // and update bookmarks accordingly.
    const hash = `OPTIMISTIC_COMMIT_${base}`;
    const description = this.message.slice(title.length);
    const info: CommitInfo = {
      author: baseInfo?.author ?? '',
      description,
      title,
      bookmarks: [],
      remoteBookmarks: [],
      isHead: true,
      parents: [base],
      hash,
      phase: 'draft',
      filesSample: this.optimisticChangedFiles,
      totalFileCount: this.optimisticChangedFiles.length,
      date: new Date(),
    };

    return dag.replaceWith([base, hash], (h, _c) => {
      if (h === base) {
        return {...baseInfo, isHead: false};
      } else {
        return info;
      }
    });
  }
}

export class PartialCommitOperation extends Operation {
  /**
   * See also `CommitOperation`. This operation takes a `PartialSelection` and
   * uses `debugimportstack` under the hood, to achieve `commit -i` effect.
   */
  constructor(
    private message: string,
    private originalHeadHash: Hash,
    private selection: PartialSelection,
    // We need "selected" or "all" files since `selection` only tracks deselected files.
    private allFiles: Array<RepoRelativePath>,
  ) {
    super('PartialCommitOperation');
  }

  getArgs(): CommandArg[] {
    return ['debugimportstack'];
  }

  getStdin(): string | undefined {
    const files = this.selection.calculateImportStackFiles(this.allFiles);
    const importStack: ImportStack = [
      [
        'commit',
        {
          mark: ':1',
          text: this.message,
          parents: [this.originalHeadHash],
          files,
        },
      ],
      ['reset', {mark: ':1'}],
    ];
    return JSON.stringify(importStack);
  }

  getDescriptionForDisplay() {
    return {
      description: t('Committing selected changes'),
      tooltip: t(
        'This operation does not have a traditional command line equivalent. \n' +
          'You can use `commit -i` on the command line to select changes to commit.',
      ),
    };
  }
}

/** Choose `PartialCommitOperation` or `CommitOperation` based on input. */
export function getCommitOperation(
  message: string,
  originalHeadHash: Hash,
  selection: PartialSelection,
  allFiles: Array<RepoRelativePath>,
): CommitOperation | PartialCommitOperation {
  if (selection.hasChunkSelection()) {
    return new PartialCommitOperation(message, originalHeadHash, selection, allFiles);
  } else if (selection.isEverythingSelected(() => allFiles)) {
    return new CommitOperation(message, originalHeadHash);
  } else {
    const selectedFiles = allFiles.filter(path => selection.isFullyOrPartiallySelected(path));
    return new CommitOperation(message, originalHeadHash, selectedFiles);
  }
}
