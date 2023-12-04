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
import type {CommandArg, Hash, RepoRelativePath, UncommittedChanges} from '../types';
import type {ImportAmendCommit, ImportStack} from 'shared/types/stack';

import {globalRecoil} from '../AccessGlobalRecoil';
import {AmendRestackBehavior, restackBehaviorAtom} from '../RestackBehavior';
import {t} from '../i18n';
import {Operation} from './Operation';

export class AmendOperation extends Operation {
  /**
   * @param filePathsToAmend if provided, only these file paths will be included in the amend operation. If undefined, ALL uncommitted changes are included. Paths should be relative to repo root.
   * @param message if provided, update commit description to use this title & description
   */
  constructor(private filePathsToAmend?: Array<RepoRelativePath>, private message?: string) {
    super('AmendOperation');

    this.restackBehavior =
      globalRecoil().getLoadable(restackBehaviorAtom).valueMaybe() ?? AmendRestackBehavior.ALWAYS;
  }

  restackBehavior: AmendRestackBehavior;

  static opName = 'Amend';

  getArgs() {
    const args: Array<CommandArg> = [
      {type: 'config', key: 'amend.autorestack', value: this.restackBehavior},
      'amend',
      '--addremove',
    ];
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
      args.push('--message', this.message);
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

  // Bump the timestamp and update the commit message.
  optimisticDag(dag: Dag): Dag {
    const head = dag.resolve('.');
    if (head?.hash == null) {
      return dag;
    }
    // XXX: amend's auto restack does not bump timestamp yet. We should fix that
    // and remove includeDescendants here.
    return dag.touch(head.hash, false /* includeDescendants */).replaceWith(head.hash, (_h, c) => {
      if (this.message == null) {
        return c;
      }
      const [title] = this.message.split(/\n+/, 1);
      const description = this.message.slice(title.length);
      // TODO: we should also update `filesSample` after amending.
      // These files are visible in the commit info view during optimistic state.
      return c && {...c, title, description};
    });
  }
}

export class PartialAmendOperation extends Operation {
  /**
   * See also `AmendOperation`. This operation takes a `PartialSelection` and
   * uses `debugimportstack` under the hood, to achieve `amend -i` effect.
   */
  constructor(
    private message: string | undefined,
    private originalHeadHash: Hash,
    private selection: PartialSelection,
    // We need "selected" or "all" files since `selection` only tracks deselected files.
    private allFiles: Array<RepoRelativePath>,
  ) {
    super('PartialAmendOperation');
  }

  getArgs(): CommandArg[] {
    return ['debugimportstack'];
  }

  getStdin(): string | undefined {
    const files = this.selection.calculateImportStackFiles(this.allFiles);
    const commitInfo: ImportAmendCommit = {
      mark: ':1',
      node: this.originalHeadHash,
      files,
    };
    if (this.message) {
      commitInfo.text = this.message;
    }
    const importStack: ImportStack = [
      ['amend', commitInfo],
      ['reset', {mark: ':1'}],
    ];
    return JSON.stringify(importStack);
  }

  getDescriptionForDisplay() {
    return {
      description: t('Amending selected changes'),
      tooltip: t(
        'This operation does not have a traditional command line equivalent. \n' +
          'You can use `amend -i` on the command line to select changes to amend.',
      ),
    };
  }
}

/** Choose `PartialAmendOperation` or `AmendOperation` based on input. */
export function getAmendOperation(
  message: string | undefined,
  originalHeadHash: Hash,
  selection: PartialSelection,
  allFiles: Array<RepoRelativePath>,
): AmendOperation | PartialAmendOperation {
  if (selection.hasChunkSelection()) {
    return new PartialAmendOperation(message, originalHeadHash, selection, allFiles);
  } else if (selection.isEverythingSelected(() => allFiles)) {
    return new AmendOperation(undefined, message);
  } else {
    const selectedFiles = allFiles.filter(path => selection.isFullyOrPartiallySelected(path));
    return new AmendOperation(selectedFiles, message);
  }
}
