/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  ApplyUncommittedChangesPreviewsFuncType,
  UncommittedChangesPreviewContext,
} from '../previews';
import type {CommandArg, RepoRelativePath, UncommittedChanges} from '../types';

import {Operation} from './Operation';

export enum ResolveTool {
  mark = 'mark',
  unmark = 'unmark',
  both = 'internal:union',
  local = 'internal:merge-local',
  other = 'internal:merge-other',
}

export class ResolveOperation extends Operation {
  constructor(private filePath: RepoRelativePath, private tool: ResolveTool) {
    super();
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
        change.path === this.filePath ? {path: change.path, status: 'Resolved'} : change,
      );
    };
    return func;
  }
}
