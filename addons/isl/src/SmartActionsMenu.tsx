/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtomValue} from 'jotai';
import {Suspense, useState} from 'react';
import {randomId} from 'shared/utils';
import {tracker} from './analytics';
import serverAPI from './ClientToServerAPI';
import {diffCommentData} from './codeReview/codeReviewAtoms';
import {diffSummary} from './codeReview/CodeReviewInfo';
import {DropdownFields} from './DropdownFields';
import {useFeatureFlagAsync, useFeatureFlagSync} from './featureFlags';
import {T} from './i18n';
import {Internal} from './Internal';
import {BaseSplitButton} from './stackEdit/ui/BaseSplitButton';
import type {CommitInfo} from './types';

import platform from './platform';
import {repositoryInfo} from './serverAPIState';
import './SmartActionsMenu.css';

export function SmartActionsMenu({commit}: {commit?: CommitInfo}) {
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
      onDismiss={() => setDropdownVisible(false)}
      group="smart-actions">
      <Button
        icon
        data-testid="smart-actions-button"
        onClick={() => tracker.track('SmartActionsMenuOpened')}
        className={'smart-actions-button' + (dropdownVisible ? ' dropdown-visible' : '')}>
        <Icon icon="lightbulb-sparkle" />
      </Button>
    </Tooltip>
  );
}

function SmartActions({commit, dismiss}: {commit?: CommitInfo; dismiss: () => void}) {
  const actions = [];

  const aiCommitSplitEnabled = useFeatureFlagAsync(Internal.featureFlags?.AICommitSplit);
  if (commit && aiCommitSplitEnabled) {
    actions.push(<AutoSplitButton key="auto-split" commit={commit} dismiss={dismiss} />);
  }

  const devmateResolveCommentsEnabled = useFeatureFlagAsync(
    Internal.featureFlags?.InlineCommentDevmateResolve,
  );
  // For now, only support this in VS Code
  if (devmateResolveCommentsEnabled && commit?.diffId && platform.platformName === 'vscode') {
    actions.push(
      <ResolveCommentsButton
        key="resolve-comments"
        diffId={commit.diffId}
        filePathsSample={commit.filePathsSample}
        dismiss={dismiss}
        disabled={!commit.isDot}
        disabledReason="This action is only available for the current commit."
      />,
    );
  }

  const devmateResolveFailedSignalsEnabled = useFeatureFlagAsync(
    Internal.featureFlags?.DevmateResolveFailedSignals,
  );
  // For now, only support this in VS Code
  if (devmateResolveFailedSignalsEnabled && commit?.diffId && platform.platformName === 'vscode') {
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

  const devmateGenerateTestsForModifiedCodeEnabled = useFeatureFlagAsync(
    Internal.featureFlags?.DevmateGenerateTestsForModifiedCode,
  );
  // For now, only support this in VS Code since the devmate can only be triggered from VS Code
  if (devmateGenerateTestsForModifiedCodeEnabled && platform.platformName === 'vscode') {
    const enabled = !commit || commit.isDot; // Enabled for `uncommitted changes` or the `current commit`.
    actions.push(
      <GenerateTestsForModifiedCodeButton
        key="generate-tests"
        dismiss={dismiss}
        disabled={!enabled}
        disabledReason="This action is only available for the current commit and uncommitted changes."
      />,
    );
  }

  const devmateGenerateCommitMessageEnabled = useFeatureFlagAsync(
    Internal.featureFlags?.DevmateGenerateCommitMessage,
  );
  // For now, only support this in VS Code
  if (!commit && devmateGenerateCommitMessageEnabled && platform.platformName === 'vscode') {
    actions.push(<FillCommitInfoButton key="fill-commit-info" dismiss={dismiss} />);
  }

  const devmateValidateChangesEnabled = useFeatureFlagAsync(
    Internal.featureFlags?.DevmateValidateChanges,
  );
  // For now, only support this in VS Code
  if (!commit && devmateValidateChangesEnabled && platform.platformName === 'vscode') {
    actions.push(<ValidateChangesButton key="validate-changes" dismiss={dismiss} />);
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
      onSplitInitiated={() => {
        tracker.track('SmartActionClicked', {extras: {action: 'AutoSplit'}});
        dismiss();
      }}>
      <Icon icon="sparkle" />
      <T>Auto-split</T>
    </BaseSplitButton>
  );
}

/** Prompt Devmate to resolve all comments on a diff. */
function ResolveCommentsButton({
  diffId,
  filePathsSample,
  dismiss,
  disabled,
  disabledReason,
}: {
  diffId: string;
  filePathsSample: readonly string[];
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
        tracker.track('SmartActionClicked', {extras: {action: 'ResolveAllComments'}});
        serverAPI.postMessage({
          type: 'platform/resolveAllCommentsWithAI',
          diffId,
          comments: diffComments.data,
          filePaths: [...filePathsSample],
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

/** Prompt Devmate to fill commit info. */
function FillCommitInfoButton({dismiss}: {dismiss: () => void}) {
  return (
    <Button
      data-testid="fill-commit-info-button"
      onClick={e => {
        tracker.track('SmartActionClicked', {extras: {action: 'FillCommitMessage'}});
        serverAPI.postMessage({
          type: 'platform/fillDevmateCommitMessage',
          id: randomId(),
          source: 'smartAction',
        });
        dismiss();
        e.stopPropagation();
      }}>
      <Icon icon="sparkle" />
      <T>Fill commit info</T>
    </Button>
  );
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
        tracker.track('SmartActionClicked', {extras: {action: 'ResolveFailedSignals'}});
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

function GenerateTestsForModifiedCodeButton({
  dismiss,
  disabled,
  disabledReason,
}: {
  dismiss: () => void;
  disabled?: boolean;
  disabledReason?: string;
}) {
  const button = (
    <Button
      data-testid="generate-tests-for-modified-code-button"
      onClick={e => {
        tracker.track('SmartActionClicked', {extras: {action: 'GenerateTests'}});
        serverAPI.postMessage({
          type: 'platform/devmateCreateTestForModifiedCode',
        });
        dismiss();
        e.stopPropagation();
      }}
      disabled={disabled}>
      <Icon icon="sparkle" />
      <T>Generate tests for changes</T>
    </Button>
  );

  return disabled ? <Tooltip title={disabledReason}>{button}</Tooltip> : button;
}

/** Prompt Devmate to validate code and fix errors in the working copy. */
function ValidateChangesButton({dismiss}: {dismiss: () => void}) {
  return (
    <Button
      data-testid="validate-changes-button"
      onClick={e => {
        tracker.track('SmartActionClicked', {extras: {action: 'ValidateChanges'}});
        serverAPI.postMessage({
          type: 'platform/devmateValidateChanges',
        });
        dismiss();
        e.stopPropagation();
      }}>
      <Icon icon="sparkle" />
      <T>Validate Changes</T>
    </Button>
  );
}
