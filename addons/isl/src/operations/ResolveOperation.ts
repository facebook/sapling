/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  ApplyMergeConflictsPreviewsFuncType,
  ApplyUncommittedChangesPreviewsFuncType,
  MergeConflictsPreviewContext,
  UncommittedChangesPreviewContext,
} from '../previews';
import type {CommandArg, MergeConflicts, RepoRelativePath, UncommittedChanges} from '../types';

import {Operation} from './Operation';

export enum ResolveTool {
  mark = 'mark',
  unmark = 'unmark',
  both = 'internal:union',
  local = 'internal:merge-local',
  other = 'internal:merge-other',
}

export class ResolveOperation extends Operation {
  constructor(
    private filePath: RepoRelativePath,
    private tool: ResolveTool,
  ) {
    super('ResolveOperation');
  }

  static opName = 'Resolve';

  getArgs() {
    const args: Array<CommandArg> = ['resolve'];

    switch (this.tool) {
      case ResolveTool.mark:
        args.push('--mark');
        break;
      case ResolveTool.unmark:
        args.push('--unmark');
        break;
      case ResolveTool.both:
      case ResolveTool.local:
      case ResolveTool.other:
        args.push('--tool', this.tool);
        break;
    }

    args.push({
      type: 'repo-relative-file' as const,
      path: this.filePath,
    });
    return args;
  }

  makeOptimisticUncommittedChangesApplier?(
    context: UncommittedChangesPreviewContext,
  ): ApplyUncommittedChangesPreviewsFuncType | undefined {
    if (
      context.uncommittedChanges.some(
        change => change.path === this.filePath && change.status !== 'U',
      )
    ) {
      return undefined;
    }

    const func: ApplyUncommittedChangesPreviewsFuncType = (changes: UncommittedChanges) => {
      return changes.map(change =>
        change.path === this.filePath ? {...change, status: 'Resolved'} : change,
      );
    };
    return func;
  }

  makeOptimisticMergeConflictsApplier?(
    context: MergeConflictsPreviewContext,
  ): ApplyMergeConflictsPreviewsFuncType | undefined {
    if (
      context.conflicts?.files?.some(
        change => change.path === this.filePath && change.status !== 'U',
      ) === true
    ) {
      return undefined;
    }

    const func: ApplyMergeConflictsPreviewsFuncType = (conflicts?: MergeConflicts) => {
      if (conflicts?.state !== 'loaded') {
        return conflicts;
      }
      return {
        ...conflicts,
        files:
          conflicts?.files?.map(change =>
            change.path === this.filePath ? {...change, status: 'Resolved' as const} : change,
          ) ?? [],
      };
    };
    return func;
  }
}
