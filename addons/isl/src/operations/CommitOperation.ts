/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ImportStack} from 'shared/types/stack';
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

import {DagCommitInfo} from '../dag/dagCommitInfo';
import {t} from '../i18n';
import {readAtom} from '../jotaiUtils';
import {uncommittedChangesWithPreviews} from '../previews';
import {authorString} from '../serverAPIState';
import {CommitBaseOperation} from './CommitBaseOperation';
import {Operation} from './Operation';

export class CommitOperation extends CommitBaseOperation {
  private beforeCommitDate: Date;

  /**
   * @param message the commit message. The first line is used as the title.
   * @param originalHeadHash the hash of the current head commit, needed to track when optimistic state is resolved.
   * @param filesPathsToCommit if provided, only these file paths will be included in the commit operation. If undefined, ALL uncommitted changes are included. Paths should be relative to repo root.
   */
  constructor(
    public message: string,
    private originalHeadHash: Hash,
    protected filesPathsToCommit?: Array<RepoRelativePath>,
  ) {
    super(message, filesPathsToCommit);

    // New commit should have a greater date.
    this.beforeCommitDate = new Date();

    // When rendering optimistic state, we need to know the set of files that will be part of this commit.
    // This is not necessarily the same as filePathsToCommit, since it may be undefined to represent "all files".
    // This is done once at Operation creation time, not on each call to optimisticDag, since we
    // only care about the list of changed files when the CommitOperation was enqueued.
    this.optimisticChangedFiles = readAtom(uncommittedChangesWithPreviews).filter(changedFile => {
      return filesPathsToCommit == null
        ? true
        : filesPathsToCommit.some(f => f === changedFile.path);
    });
  }

  private optimisticChangedFiles: Array<ChangedFile>;

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

    const now = new Date(Date.now());

    // The fake optimistic commit can be resolved into a real commit by taking the
    // first child of the given parent that's created after the commit operation was created.
    const optimisticRevset = `first(sort((children(${base})-${base}) & date(">${now.toUTCString()}"),date))`;

    // NOTE: We might want to check the "active bookmark" state
    // and update bookmarks accordingly.
    const hash = `OPTIMISTIC_COMMIT_${base}`;
    const description = this.message.slice(title.length);
    const author = readAtom(authorString);
    const info = DagCommitInfo.fromCommitInfo({
      author: author ?? baseInfo?.author ?? '',
      description,
      title,
      bookmarks: [],
      remoteBookmarks: [],
      isDot: true,
      parents: [base],
      hash,
      optimisticRevset,
      phase: 'draft',
      filePathsSample: this.optimisticChangedFiles.map(f => f.path),
      totalFileCount: this.optimisticChangedFiles.length,
      date: now,
    });

    return dag.replaceWith([base, hash], (h, _c) => {
      if (h === base) {
        return baseInfo?.set('isDot', false);
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
    public message: string,
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
  originalHead: CommitInfo | undefined,
  selection: PartialSelection,
  allFiles: Array<RepoRelativePath>,
): CommitOperation | PartialCommitOperation {
  const originalHeadHash = originalHead?.hash ?? '.';
  if (selection.hasChunkSelection()) {
    return new PartialCommitOperation(message, originalHeadHash, selection, allFiles);
  } else if (selection.isEverythingSelected(() => allFiles)) {
    return new CommitOperation(message, originalHeadHash);
  } else {
    const selectedFiles = allFiles.filter(path => selection.isFullyOrPartiallySelected(path));
    return new CommitOperation(message, originalHeadHash, selectedFiles);
  }
}
