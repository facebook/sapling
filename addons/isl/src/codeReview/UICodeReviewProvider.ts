/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Operation} from '../operations/Operation';
import type {Dag} from '../previews';
import type {CommitInfo, DiffId, DiffSummary, Hash} from '../types';
import type {SyncStatus} from './syncStatus';
import type {ReactNode} from 'react';

/**
 * API to interact with Code Review for Repositories, e.g. GitHub and Phabricator.
 */
export interface UICodeReviewProvider {
  name: string;
  label: string;

  /** name used to run commands provider-specific commands */
  cliName?: string;

  DiffBadgeContent(props: {
    diff?: DiffSummary;
    children?: ReactNode;
    syncStatus?: SyncStatus;
  }): JSX.Element | null;
  /** If this provider is capable of landing from the UI, this component renders the land button */
  DiffLandButtonContent?(props: {diff?: DiffSummary; commit: CommitInfo}): JSX.Element | null;
  formatDiffNumber(diffId: DiffId): string;

  submitOperation(
    commits: Array<CommitInfo>,
    options?: {
      /** Whether to submit this diff as a draft. Note: some review providers only allow submitting new Diffs as drafts */
      draft?: boolean;
      /** If this diff is being resubmitted, this message will be added as a comment to explain what has changed */
      updateMessage?: string;
      /** Whether to update the remote message with the local commit message */
      updateFields?: boolean;
    },
  ): Operation;

  submitCommandName(): string;

  RepoInfo(): JSX.Element | null;

  isDiffClosed(summary: DiffSummary): boolean;

  isDiffEligibleForCleanup(summary: DiffSummary): boolean;

  getSyncStatuses(
    commits: Array<CommitInfo>,
    allDiffSummaries: Map<string, DiffSummary>,
  ): Map<Hash, SyncStatus>;

  /**
   * Defines when this review provider can submit diffs as drafts,
   * submitting for the first time or also when resubmitting.
   */
  supportSubmittingAsDraft: 'always' | 'newDiffsOnly';
  /** Whether this review provider allows attaching a short update message when resubmitting a diff. */
  supportsUpdateMessage: boolean;

  getSupportedStackActions(
    hash: Hash,
    dag: Dag,
    diffSummaries: Map<string, DiffSummary>,
  ): {
    resubmittableStack?: Array<CommitInfo>;
    submittableStack?: Array<CommitInfo>;
  };

  /**
   * Given a set of a DiffSummaries, return which ones are ad-hoc submittable by this provider,
   * meaning you don't need to change the working copy to submit them.
   */
  getSubmittableDiffs(
    commits: Array<CommitInfo>,
    allDiffSummaries: Map<string, DiffSummary>,
  ): Array<CommitInfo>;

  enableMessageSyncing: boolean;

  supportsSuggestedReviewers: boolean;

  supportsComparingSinceLastSubmit: boolean;

  supportsRenderingMarkup: boolean;
}
