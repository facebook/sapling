/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export type TrackEventName =
  | 'ClickedRefresh'
  | 'ClientConnection'
  | 'LoadMoreCommits'
  | 'RunOperation'
  | 'StarRating'
  | 'TopLevelErrorShown'
  | 'UIEmptyState'
  | 'AbortMergeOperation'
  | 'PullOperation'
  | 'AbortMergeOperation'
  | 'AddOperation'
  | 'AddRemoveOperation'
  | 'AmendMessageOperation'
  | 'AmendOperation'
  | 'CommitOperation'
  | 'ContinueMergeOperation'
  | 'CreateEmptyInitialCommit'
  | 'DiscardOperation'
  | 'ForgetOperation'
  | 'GhStackSubmitOperation'
  | 'GotoOperation'
  | 'HideOperation'
  | 'PrSubmitOperation'
  | 'PullOperation'
  | 'PurgeOperation'
  | 'RebaseOperation'
  | 'ResolveOperation'
  | 'RevertOperation'
  | 'SetConfigOperation'
  | 'UncommitOperation'
  // @fb-only
  | 'OptimisticFilesStateForceResolved'
  | 'OptimisticCommitsStateForceResolved'
  | 'OptimisticConflictsStateForceResolved'
  | 'QueueOperation'
  | 'UnsubmittedStarRating'
  | 'UploadImage'
  | 'RunVSCodeCommand'
  | 'UnsubmittedStarRating'
  | 'VSCodeExtensionActivated';

export type TrackErrorName =
  | 'DiffFetchFailed'
  | 'InvalidCwd'
  | 'InvalidCommand'
  | 'GhCliNotAuthenticated'
  | 'GhCliNotInstalled'
  | 'TopLevelError'
  | 'RunOperationError'
  | 'RepositoryError'
  | 'UploadImageError'
  | 'VSCodeCommandError'
  | 'VSCodeActivationError';
