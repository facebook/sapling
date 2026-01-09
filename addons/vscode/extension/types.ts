/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/** Not all features of the VS Code API may be enabled / rolled out, so they are controlled individually.
 * In OSS, they are all enabled. Internally, they may be disabled while transitioning from an older system.
 * blame => inline and toggleable blame
 * sidebar => VS Code SCM API, VS Code Source Control sidebar entry.
 * diffview => diff commands, gutters. Requires 'sidebar'.
 * */
export type EnabledSCMApiFeature =
  | 'blame'
  | 'sidebar'
  | 'comments'
  | 'newInlineComments'
  | 'aiFirstPassCodeReview';

export enum ActionTriggerType {
  ISL2InlineComment = 'ISL2InlineComment', // provided from the Sapling ISL Inline Comment
  ISL2SmartActions = 'ISL2SmartActions', // provided from the Sapling ISL Smart Actions Menu
  ISL2CommitInfoView = 'ISL2CommitInfoView', // provided from the Sapling ISL Commit Info View
  ISL2MergeConflictView = 'ISL2MergeConflictView', // provided from the Sapling ISL Merge Conflict View
  ISL2SplitCommit = 'ISL2SplitCommit', // provided from the Sapling ISL Split Commit UI
}
