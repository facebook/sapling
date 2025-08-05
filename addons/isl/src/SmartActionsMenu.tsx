/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {DropdownFields} from './DropdownFields';
import {featureFlagAsync, useFeatureFlagSync} from './featureFlags';
import {T} from './i18n';
import {Internal} from './Internal';
import {BaseSplitButton} from './stackEdit/ui/BaseSplitButton';
import type {CommitInfo} from './types';
import {Suspense, useState} from 'react';
import {diffCommentData} from './codeReview/codeReviewAtoms';
import {diffSummary} from './codeReview/CodeReviewInfo';
import {useAtomValue} from 'jotai';
import serverAPI from './ClientToServerAPI';

import './SmartActionsMenu.css';
import platform from './platform';
import {repositoryInfo} from './serverAPIState';

export function SmartActionsMenu({commit}: {commit: CommitInfo}) {
  const [dropdownVisible, setDropdownVisible] = useState(false);

  const smartActionsMenuEnabled = useFeatureFlagSync(Internal.featureFlags?.SmartActionsMenu);
  if (!smartActionsMenuEnabled) {
    return null;
  }

  return (
    <Tooltip
      component={dismiss => {
        return (
          <Suspense fallback={<Icon icon="loading" />}>
            <SmartActions commit={commit} dismiss={dismiss} />
          </Suspense>
        );
      }}
      trigger="click"
      title={<T>Smart Actions...</T>}
      onVisible={() => setDropdownVisible(true)}
      onDismiss={() => setDropdownVisible(false)}>
      <Button
        icon
        data-testid="smart-actions-button"
        className={'smart-actions-button' + (dropdownVisible ? ' dropdown-visible' : '')}>
        <Icon icon="lightbulb-sparkle" />
      </Button>
    </Tooltip>
  );
}

function SmartActions({commit, dismiss}: {commit: CommitInfo; dismiss: () => void}) {
  const actions = [];

  const aiCommitSplitEnabled = useAtomValue(featureFlagAsync(Internal.featureFlags?.AICommitSplit));
  if (aiCommitSplitEnabled) {
    actions.push(<AutoSplitButton key="auto-split" commit={commit} dismiss={dismiss} />);
  }

  const devmateResolveCommentsEnabled = useAtomValue(
    featureFlagAsync(Internal.featureFlags?.InlineCommentDevmateResolve),
  );
  // For now, only support this in VS Code
  if (devmateResolveCommentsEnabled && commit.diffId && platform.platformName === 'vscode') {
    actions.push(
      <ResolveCommentsButton
        key="resolve-comments"
        diffId={commit.diffId}
        dismiss={dismiss}
        disabled={!commit.isDot}
        disabledReason="This action is only available for the current commit."
      />,
    );
  }

  const devmateResolveFailedSignalsEnabled = useAtomValue(
    featureFlagAsync(Internal.featureFlags?.DevmateResolveFailedSignals),
  );
  // For now, only support this in VS Code
  if (devmateResolveFailedSignalsEnabled && commit.diffId && platform.platformName === 'vscode') {
    actions.push(
      <ResolveFailedSignalsButton
        key="resolve-failed-signals"
        diffId={commit.diffId}
        dismiss={dismiss}
        disabled={!commit.isDot}
        disabledReason="This action is only available for the current commit."
      />,
    );
  }

  return (
    <DropdownFields
      title={<T>Smart Actions</T>}
      icon="lightbulb-sparkle"
      className="smart-actions-dropdown"
      data-testid="smart-actions-dropdown">
      {actions.length > 0 ? actions : <T>No smart actions available</T>}
    </DropdownFields>
  );
}

/** Like SplitButton, but triggers AI split automatically. */
function AutoSplitButton({commit, dismiss}: {commit: CommitInfo; dismiss: () => void}) {
  return (
    <BaseSplitButton
      commit={commit}
      trackerEventName="SplitOpenFromSmartActions"
      autoSplit={true}
      onSplitInitiated={dismiss}>
      <Icon icon="sparkle" />
      <T>Auto-split</T>
    </BaseSplitButton>
  );
}

/** Prompt Devmate to resolve all comments on a diff. */
function ResolveCommentsButton({
  diffId,
  dismiss,
  disabled,
  disabledReason,
}: {
  diffId: string;
  dismiss: () => void;
  disabled?: boolean;
  disabledReason?: string;
}) {
  const repo = useAtomValue(repositoryInfo);
  const repoPath = repo?.repoRoot;
  const diffComments = useAtomValue(diffCommentData(diffId));
  if (diffComments.state === 'loading') {
    return <Icon icon="loading" />;
  }
  if (diffComments.state === 'hasError' || diffComments.data.length === 0) {
    return;
  }

  const button = (
    <Button
      data-testid="review-comments-button"
      onClick={e => {
        serverAPI.postMessage({
          type: 'platform/resolveAllCommentsWithAI',
          diffId,
          comments: diffComments.data,
          repoPath,
        });
        dismiss();
        e.stopPropagation();
      }}
      disabled={disabled}>
      <Icon icon="sparkle" />
      <T>Resolve all comments</T>
    </Button>
  );

  return disabled ? <Tooltip title={disabledReason}>{button}</Tooltip> : button;
}

/** Prompt Devmate to resolve failed signals on a diff. */
function ResolveFailedSignalsButton({
  diffId,
  dismiss,
  disabled,
  disabledReason,
}: {
  diffId: string;
  dismiss: () => void;
  disabled?: boolean;
  disabledReason?: string;
}) {
  const repo = useAtomValue(repositoryInfo);
  const repoPath = repo?.repoRoot;
  const diffSummaryResult = useAtomValue(diffSummary(diffId));

  // Only show the button if there are failed signals
  if (diffSummaryResult.error) {
    return null;
  }
  if (
    !diffSummaryResult.value?.signalSummary ||
    diffSummaryResult.value.signalSummary !== 'failed'
  ) {
    return null;
  }

  const button = (
    <Button
      data-testid="resolve-failed-signals-button"
      onClick={e => {
        serverAPI.postMessage({
          type: 'platform/resolveFailedSignalsWithAI',
          diffId,
          repoPath,
        });
        dismiss();
        e.stopPropagation();
      }}
      disabled={disabled}>
      <Icon icon="sparkle" />
      <T>Fix failed signals</T>
    </Button>
  );

  return disabled ? <Tooltip title={disabledReason}>{button}</Tooltip> : button;
}
