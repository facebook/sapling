/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TrackEventName} from 'isl-server/src/analytics/eventNames';
import type {
  ApplyMergeConflictsPreviewsFuncType,
  ApplyUncommittedChangesPreviewsFuncType,
  Dag,
  MergeConflictsPreviewContext,
  UncommittedChangesPreviewContext,
} from '../previews';
import type {CommandArg, RunnableOperation} from '../types';

import {randomId} from 'shared/utils';
import {enableSaplingDebugFlag, enableSaplingVerboseFlag} from '../atoms/debugToolAtoms';
import {readAtom} from '../jotaiUtils';
import {CommandRunner} from '../types';

/**
 * Operations represent commands that mutate the repository, such as rebasing, committing, etc.
 * Operations are intended to be relatively long-lived processses which show progress, are cancellable, and must be run one-at-a-time.
 * This is as opposed to other commands like status, log, cat, which may be run in parallel and do not (necessarily) show stdout progress.
 * You can get arguments, get the preview applier function, get the optimistic state applier function, get documentation, etc.
 */
export abstract class Operation {
  static operationName: string;
  public id: string = randomId();

  constructor(public trackEventName: TrackEventName) {}

  abstract getArgs(): Array<CommandArg>;

  /** Optional stdin data piped to the process. */
  getStdin(): string | undefined {
    return undefined;
  }

  /** Description of the operation. This can replace the default display. */
  getDescriptionForDisplay(): OperationDescription | undefined {
    return undefined;
  }

  /**
   * When the operation starts running, prefill inline progress messages to show up next to one or more commits.
   * Note: most operations/runners never report additional inline progress, meaning this is typically shown for the duration of the operation.
   */
  getInitialInlineProgress?(): Array<[hash: string, message: string]>;

  public runner: CommandRunner = CommandRunner.Sapling;

  makeOptimisticUncommittedChangesApplier?(
    context: UncommittedChangesPreviewContext,
  ): ApplyUncommittedChangesPreviewsFuncType | undefined;

  makeOptimisticMergeConflictsApplier?(
    context: MergeConflictsPreviewContext,
  ): ApplyMergeConflictsPreviewsFuncType | undefined;

  /** Effects to `dag` before confirming the operation. */
  previewDag(dag: Dag): Dag {
    return dag;
  }

  /**
   * Effects to `dag` after confirming the operation.
   * The operation is running or queued.
   */
  optimisticDag(dag: Dag): Dag {
    return dag;
  }

  getRunnableOperation(): RunnableOperation {
    return {
      args: [...this.getExtraArgs(), ...this.getArgs()],
      id: this.id,
      stdin: this.getStdin(),
      runner: this.runner,
      trackEventName: this.trackEventName,
    };
  }

  /** Extra global args, like --debug, --verbose. */
  getExtraArgs(): Array<CommandArg> {
    const args: Array<CommandArg> = [];
    if (this.runner === CommandRunner.Sapling) {
      if (readAtom(enableSaplingVerboseFlag)) {
        args.push('--verbose');
      }
      if (readAtom(enableSaplingDebugFlag)) {
        args.push('--debug');
      }
    }
    return args;
  }
}

/** Access static opName field of an operation */
export function getOpName(op: Operation): string {
  return (op.constructor as unknown as {opName: string}).opName;
}

/** Descirbe how to display a operation. */
export type OperationDescription = {
  /** If set, this replaces the default command arguments. */
  description?: string;

  /**
   * If set, this replaces the default command in the output tooltip.
   * It also indicates that the output lines might contain a JSON string
   * that is not suitable for human reading.
   */
  tooltip?: string;
};
