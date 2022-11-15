/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {EditedMessage} from '../CommitInfo';
import type {
  ApplyPreviewsFuncType,
  ApplyUncommittedChangesPreviewsFuncType,
  PreviewContext,
  UncommittedChangesPreviewContext,
} from '../previews';
import type {CommandArg, RepoRelativePath, UncommittedChanges} from '../types';

import {Operation} from './Operation';

export class AmendOperation extends Operation {
  /**
   * @param filePathsToAmend if provided, only these file paths will be included in the amend operation. If undefined, ALL uncommitted changes are included. Paths should be relative to repo root.
   * @param message if provided, update commit description to use this title & description
   */
  constructor(private filePathsToAmend?: Array<RepoRelativePath>, private message?: EditedMessage) {
    super();
  }

  static opName = 'Amend';

  getArgs() {
    const args: Array<CommandArg> = ['amend'];
    if (this.filePathsToAmend) {
      args.push(
        ...this.filePathsToAmend.map(file =>
          // tag file arguments specialy so the remote repo can convert them to the proper cwd-relative format.
          ({
            type: 'repo-relative-file' as const,
            path: file,
          }),
        ),
      );
    }
    if (this.message) {
      args.push('--message', `${this.message.title}\n${this.message.description}`);
    }
    return args;
  }

  makeOptimisticUncommittedChangesApplier?(
    context: UncommittedChangesPreviewContext,
  ): ApplyUncommittedChangesPreviewsFuncType | undefined {
    const filesToAmend = new Set(this.filePathsToAmend);
    if (
      context.uncommittedChanges.length === 0 ||
      (filesToAmend.size > 0 &&
        context.uncommittedChanges.every(change => !filesToAmend.has(change.path)))
    ) {
      return undefined;
    }

    const func: ApplyUncommittedChangesPreviewsFuncType = (changes: UncommittedChanges) => {
      if (this.filePathsToAmend != null) {
        return changes.filter(change => !filesToAmend.has(change.path));
      } else {
        return [];
      }
    };
    return func;
  }

  // optimistic state is only minorly useful for amend:
  // we just need it to update the head commit's title/description
  makeOptimisticApplier(context: PreviewContext): ApplyPreviewsFuncType | undefined {
    const head = context.headCommit;
    if (this.message == null) {
      return undefined;
    }
    if (head?.title === this.message.title && head?.description === this.message.description) {
      // amend succeeded when the message is what we asked for
      return undefined;
    }

    const func: ApplyPreviewsFuncType = (tree, _previewType) => {
      if (tree.info.isHead) {
        // use fake title/description on the head commit
        return {
          // TODO: we should also update `filesSample` after amending.
          // These files are visible in the commit info view during optimistic state.
          // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
          info: {...tree.info, title: this.message!.title, description: this.message!.description},
          children: tree.children,
        };
      } else {
        return {info: tree.info, children: tree.children};
      }
    };
    return func;
  }
}
