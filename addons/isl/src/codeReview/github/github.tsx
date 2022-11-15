/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Operation} from '../../operations/Operation';
import type {DiffId, DiffSummary, PreferredSubmitCommand} from '../../types';
import type {UICodeReviewProvider} from '../UICodeReviewProvider';
import type {ReactNode} from 'react';

import {Icon} from '../../Icon';
import {Tooltip} from '../../Tooltip';
import {t, T} from '../../i18n';
import {GhStackSubmitOperation} from '../../operations/GhStackSubmitOperation';
import {PrSubmitOperation} from '../../operations/PrSubmitOperation';
import {PullRequestState} from 'isl-server/src/github/generated/graphql';

import './GitHubPRBadge.css';

export class GithubUICodeReviewProvider implements UICodeReviewProvider {
  name = 'github';

  constructor(private preferredSubmitCommand: PreferredSubmitCommand) {}

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
      <div
        className={`github-diff-status${diff?.state ? ' github-diff-status-' + diff.state : ''}`}>
        <Tooltip title={t('Click to open Pull Request in GitHub')} delayMs={500}>
          {diff && <Icon icon={iconForPRState(diff.state)} />}
          {diff?.state && <PRStateLabel state={diff.state} />}
          {children}
        </Tooltip>
      </div>
    );
  }

  formatDiffNumber(diffId: DiffId): string {
    return `#${diffId}`;
  }

  submitOperation(): Operation {
    if (this.preferredSubmitCommand === 'ghstack') {
      return new GhStackSubmitOperation();
    }
    return new PrSubmitOperation();
  }
}

type BadgeState = PullRequestState | 'ERROR';

function iconForPRState(state?: BadgeState) {
  switch (state) {
    case 'ERROR':
      return 'error';
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
    case 'ERROR':
      return <T>Error</T>;
    default:
      return <T>{state}</T>;
  }
}
