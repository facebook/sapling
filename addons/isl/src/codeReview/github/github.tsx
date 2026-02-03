/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';
import type {Operation} from '../../operations/Operation';
import type {
  CodeReviewSystem,
  CommitInfo,
  DiffId,
  DiffSummary,
  PreferredSubmitCommand,
} from '../../types';
import type {UICodeReviewProvider} from '../UICodeReviewProvider';
import type {SyncStatus} from '../syncStatus';

import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {PullRequestReviewDecision, PullRequestState} from 'isl-server/src/github/generated/graphql';
import {MS_PER_DAY} from 'shared/constants';
import {OSSCommitMessageFieldSchema} from '../../CommitInfoView/OSSCommitMessageFieldsSchema';
import {Internal} from '../../Internal';
import {t, T} from '../../i18n';
import {GhStackSubmitOperation} from '../../operations/GhStackSubmitOperation';
import {PrSubmitOperation} from '../../operations/PrSubmitOperation';

import './GitHubPRBadge.css';

export class GithubUICodeReviewProvider implements UICodeReviewProvider {
  name = 'github';
  label = t('GitHub');

  constructor(
    public system: CodeReviewSystem & {type: 'github'},
    private preferredSubmitCommand: PreferredSubmitCommand,
  ) {}
  cliName?: string | undefined;

  DiffBadgeContent({
    diff,
    children,
  }: {
    diff?: DiffSummary;
    children?: ReactNode;
  }): JSX.Element | null {
    if (diff != null && diff?.type !== 'github') {
      return null;
    }
    return (
      <div className="github-diff-info">
        <div
          className={
            'github-diff-status' + (diff?.state ? ` github-diff-status-${diff.state}` : '')
          }>
          <Tooltip title={t('Click to open Pull Request in GitHub')} delayMs={500}>
            {diff && <Icon className="github-diff-badge-icon" icon={iconForPRState(diff.state)} />}
            {diff?.state && <PRStateLabel state={diff.state} />}
            {children}
          </Tooltip>
        </div>
        {diff?.reviewDecision && <ReviewDecision decision={diff.reviewDecision} />}
      </div>
    );
  }

  formatDiffNumber(diffId: DiffId): string {
    return `#${diffId}`;
  }

  getSyncStatuses(
    _commits: CommitInfo[],
    _allDiffSummaries: Map<string, DiffSummary>,
  ): Map<string, SyncStatus> {
    // TODO: support finding the sync status for GitHub PRs
    return new Map();
  }

  RepoInfo = () => {
    return (
      <span>
        {this.system.hostname !== 'github.com' ? this.system.hostname : ''} {this.system.owner}/
        {this.system.repo}
      </span>
    );
  };

  getRemoteTrackingBranch(): string | null {
    return null;
  }

  getRemoteTrackingBranchFromDiffSummary(): string | null {
    return null;
  }

  isSplitSuggestionSupported(): boolean {
    return false;
  }
  submitOperation(
    _commits: Array<CommitInfo>,
    options: {draft?: boolean; updateMessage?: string; publishWhenReady?: boolean},
  ): Operation {
    if (this.preferredSubmitCommand === 'ghstack') {
      return new GhStackSubmitOperation(options);
    } else if (this.preferredSubmitCommand === 'pr') {
      return new PrSubmitOperation(options);
    } else {
      throw new Error('Not yet implemented');
    }
  }

  submitCommandName() {
    return `sl ${this.preferredSubmitCommand}`;
  }

  getSupportedStackActions() {
    return {};
  }

  getSubmittableDiffs() {
    return [];
  }

  isDiffClosed(diff: DiffSummary & {type: 'github'}): boolean {
    return diff.state === PullRequestState.Closed;
  }

  isDiffEligibleForCleanup(diff: DiffSummary & {type: 'github'}): boolean {
    return diff.state === PullRequestState.Closed;
  }

  getUpdateDiffActions(_summary: DiffSummary) {
    return [];
  }

  commitMessageFieldsSchema =
    Internal.CommitMessageFieldSchemaForGitHub ?? OSSCommitMessageFieldSchema;

  supportSubmittingAsDraft = 'newDiffsOnly' as const;
  supportsUpdateMessage = false;
  submitDisabledReason = () =>
    Internal.submitForGitHubDisabledReason?.(this.preferredSubmitCommand);
  supportBranchingPrs = true;

  branchNameForRemoteBookmark(bookmark: string) {
    // TODO: is "origin" really always the prefix for remote bookmarks in git?
    const originPrefix = 'origin/';
    const branchName = bookmark.startsWith(originPrefix)
      ? bookmark.slice(originPrefix.length)
      : bookmark;
    return branchName;
  }

  enableMessageSyncing = false;

  supportsSuggestedReviewers = false;

  supportsComparingSinceLastSubmit = false;

  supportsRenderingMarkup = false;

  gotoDistanceWarningAgeCutoff = 30 * MS_PER_DAY;
}

type BadgeState = PullRequestState | 'ERROR' | 'DRAFT' | 'MERGE_QUEUED';

function iconForPRState(state?: BadgeState) {
  switch (state) {
    case 'ERROR':
      return 'error';
    case 'DRAFT':
    case 'MERGE_QUEUED':
      return 'git-pull-request';
    case PullRequestState.Open:
      return 'git-pull-request';
    case PullRequestState.Merged:
      return 'git-merge';
    case PullRequestState.Closed:
      return 'git-pull-request-closed';
    default:
      return 'git-pull-request';
  }
}

function PRStateLabel({state}: {state: BadgeState}) {
  switch (state) {
    case PullRequestState.Open:
      return <T>Open</T>;
    case PullRequestState.Merged:
      return <T>Merged</T>;
    case PullRequestState.Closed:
      return <T>Closed</T>;
    case 'DRAFT':
      return <T>Draft</T>;
    case 'ERROR':
      return <T>Error</T>;
    case 'MERGE_QUEUED':
      return <T>Merge Queued</T>;
    default:
      return <T>{state}</T>;
  }
}

function reviewDecisionLabel(decision: PullRequestReviewDecision) {
  switch (decision) {
    case PullRequestReviewDecision.Approved:
      return <T>Approved</T>;
    case PullRequestReviewDecision.ChangesRequested:
      return <T>Changes Requested</T>;
    case PullRequestReviewDecision.ReviewRequired:
      return <T>Review Required</T>;
    default:
      return <T>{decision}</T>;
  }
}

function ReviewDecision({decision}: {decision: PullRequestReviewDecision}) {
  return (
    <div className={`github-review-decision github-review-decision-${decision}`}>
      {reviewDecisionLabel(decision)}
    </div>
  );
}
