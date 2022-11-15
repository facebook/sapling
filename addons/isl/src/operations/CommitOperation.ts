/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {EditedMessage} from '../CommitInfo';
import type {CommitTree} from '../getCommitTree';
import type {
  ApplyPreviewsFuncType,
  ApplyUncommittedChangesPreviewsFuncType,
  PreviewContext,
  UncommittedChangesPreviewContext,
} from '../previews';
import type {CommandArg, Hash, RepoRelativePath, UncommittedChanges} from '../types';

import {Operation} from './Operation';

export class CommitOperation extends Operation {
  /**
   * @param message the commit message. The first line is used as the title.
   * @param originalHeadHash the hash of the current head commit, needed to track when optimistic state is resolved.
   * @param filesPathsToCommit if provided, only these file paths will be included in the commit operation. If undefined, ALL uncommitted changes are included. Paths should be relative to repo root.
   */
  constructor(
    private message: EditedMessage,
    private originalHeadHash: Hash,
    private filesPathsToCommit?: Array<RepoRelativePath>,
  ) {
    super();
  }

  static opName = 'Commit';

  getArgs() {
    const args: Array<CommandArg> = [
      'commit',
      '--message',
      `${this.message.title}\n${this.message.description}`,
    ];
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

    const optimisticCommit: CommitTree = {
      children: [],
      info: {
        author: head?.author ?? '',
        description: this.message.description,
        title: this.message.title,
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
