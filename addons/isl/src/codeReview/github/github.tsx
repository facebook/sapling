/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

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
import type {ReactNode} from 'react';

import {Tooltip} from '../../Tooltip';
import {t, T} from '../../i18n';
import {GhStackSubmitOperation} from '../../operations/GhStackSubmitOperation';
import {PrSubmitOperation} from '../../operations/PrSubmitOperation';
import {PullRequestReviewDecision, PullRequestState} from 'isl-server/src/github/generated/graphql';
import {Icon} from 'shared/Icon';

import './GitHubPRBadge.css';

export class GithubUICodeReviewProvider implements UICodeReviewProvider {
  name = 'github';
  label = t('GitHub');

  constructor(
    private system: CodeReviewSystem & {type: 'github'},
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
            {diff && <Icon icon={iconForPRState(diff.state)} />}
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

  submitOperation(_commits: [], options: {draft?: boolean; updateMessage?: string}): Operation {
    if (this.preferredSubmitCommand === 'ghstack') {
      return new GhStackSubmitOperation(options);
    }
    return new PrSubmitOperation(options);
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

  supportSubmittingAsDraft = 'newDiffsOnly' as const;
  supportsUpdateMessage = false;

  enableMessageSyncing = false;

  supportsSuggestedReviewers = false;

  supportsComparingSinceLastSubmit = false;

  supportsRenderingMarkup = false;
}

type BadgeState = PullRequestState | 'ERROR' | 'DRAFT';

function iconForPRState(state?: BadgeState) {
  switch (state) {
    case 'ERROR':
      return 'error';
    case 'DRAFT':
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
