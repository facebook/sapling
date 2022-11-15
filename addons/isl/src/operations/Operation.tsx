/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  ApplyPreviewsFuncType,
  ApplyUncommittedChangesPreviewsFuncType,
  PreviewContext,
  UncommittedChangesPreviewContext,
} from '../previews';
import type {CommandArg} from '../types';

import {CommandRunner} from '../types';
import {randomId} from 'shared/utils';

/**
 * Operations represent commands that mutate the repository, such as rebasing, committing, etc.
 * Operations are intended to be relatively long-lived processses which show progress, are cancellable, and must be run one-at-a-time.
 * This is as opposed to other commands like status, log, cat, which may be run in parallel and do not (necessarily) show stdout progress.
 * You can get arguments, get the preview applier function, get the optimistic state applier function, get documentation, etc.
 */
export abstract class Operation {
  static operationName: string;
  public id: string = randomId();

  abstract getArgs(): Array<CommandArg>;

  public runner: CommandRunner = CommandRunner.Sapling;

  /** Used to preview how this operation would affect the tree, if you ran it. */
  makePreviewApplier?(context: PreviewContext): ApplyPreviewsFuncType | undefined;

  /** Used to show how this operation will affect the tree, after it's finished running and we get new data from hg. */
  makeOptimisticApplier?(context: PreviewContext): ApplyPreviewsFuncType | undefined;

  makeOptimisticUncommittedChangesApplier?(
    context: UncommittedChangesPreviewContext,
  ): ApplyUncommittedChangesPreviewsFuncType | undefined;
}
