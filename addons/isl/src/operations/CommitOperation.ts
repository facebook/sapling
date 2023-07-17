/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitTree} from '../getCommitTree';
import type {PartialSelection} from '../partialSelection';
import type {
  ApplyPreviewsFuncType,
  ApplyUncommittedChangesPreviewsFuncType,
  PreviewContext,
  UncommittedChangesPreviewContext,
} from '../previews';
import type {CommandArg, Hash, RepoRelativePath, UncommittedChanges} from '../types';
import type {ImportStack} from 'shared/types/stack';

import {t} from '../i18n';
import {Operation} from './Operation';

export class CommitOperation extends Operation {
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
  }

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

  makeOptimisticApplier(context: PreviewContext): ApplyPreviewsFuncType | undefined {
    const OPTIMISTIC_COMMIT_HASH = 'OPTIMISTIC_COMMIT_HASH';
    const head = context.headCommit;
    if (head?.hash !== this.originalHeadHash) {
      // commit succeeded when we no longer see the original head hash
      return undefined;
    }

    const [title] = this.message.split(/\n+/, 1);
    const description = this.message.slice(title.length);

    const optimisticCommit: CommitTree = {
      children: [],
      info: {
        author: head?.author ?? '',
        description: description ?? '',
        title,
        bookmarks: [],
        remoteBookmarks: [],
        // TODO: we should include the files that will be in the commit.
        // These files are visible in the commit info view during optimistic state.
        filesSample: [],
        isHead: true,
        parents: [head?.hash ?? ''],
        hash: OPTIMISTIC_COMMIT_HASH,
        phase: 'draft',
        totalFileCount: 0,
        date: new Date(),
      },
    };
    const func: ApplyPreviewsFuncType = (tree, _previewType) => {
      if (tree.info.hash === this.originalHeadHash) {
        // insert fake commit as a child of the old head
        return {
          info: {...tree.info, isHead: false}, // overwrite previous head as no longer being head
          children: [...tree.children, optimisticCommit],
        };
      } else {
        return {info: tree.info, children: tree.children};
      }
    };
    return func;
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
