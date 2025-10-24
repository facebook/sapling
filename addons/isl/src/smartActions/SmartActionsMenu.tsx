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
import {tracker} from '../analytics';
import serverAPI from '../ClientToServerAPI';
import {diffCommentData} from '../codeReview/codeReviewAtoms';
import {diffSummary} from '../codeReview/CodeReviewInfo';
import {DropdownFields} from '../DropdownFields';
import {useFeatureFlagAsync, useFeatureFlagSync} from '../featureFlags';
import {T} from '../i18n';
import {Internal} from '../Internal';
import {BaseSplitButton} from '../stackEdit/ui/BaseSplitButton';
import type {CommitInfo} from '../types';

import platform from '../platform';
import {repositoryInfo} from '../serverAPIState';
import './SmartActionsMenu.css';

export function SmartActionsMenu({commit}: {commit?: CommitInfo}) {
  const [dropdownVisible, setDropdownVisible] = useState(false);

  const smartActionsMenuEnabled = useFeatureFlagSync(Internal.featureFlags?.SmartActionsMenu);
  if (!smartActionsMenuEnabled || !Internal.smartActions?.showSmartActions) {
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

  const aiResolveCommentsEnabled = useFeatureFlagAsync(
    Internal.featureFlags?.InlineCommentAIResolve,
  );
  // For now, only support this in VS Code
  if (aiResolveCommentsEnabled && commit?.diffId && platform.platformName === 'vscode') {
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

  const aiResolveFailedSignalsEnabled = useFeatureFlagAsync(
    Internal.featureFlags?.AIResolveFailedSignals,
  );
  // For now, only support this in VS Code
  if (aiResolveFailedSignalsEnabled && commit?.diffId && platform.platformName === 'vscode') {
    actions.push(
      <ResolveFailedSignalsButton
        key="resolve-failed-signals"
        hash={commit.hash}
        diffId={commit.diffId}
        dismiss={dismiss}
        disabled={!commit.isDot}
        disabledReason="This action is only available for the current commit."
      />,
    );
  }

  const aiGenerateTestsForModifiedCodeEnabled = useFeatureFlagAsync(
    Internal.featureFlags?.AIGenerateTestsForModifiedCode,
  );
  // For now, only support this in VS Code
  if (aiGenerateTestsForModifiedCodeEnabled && platform.platformName === 'vscode') {
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

  const aiGenerateCommitMessageEnabled = useFeatureFlagAsync(
    Internal.featureFlags?.AIGenerateCommitMessage,
  );
  // For now, only support this in VS Code
  if (!commit && aiGenerateCommitMessageEnabled && platform.platformName === 'vscode') {
    actions.push(<FillCommitInfoButton key="fill-commit-info" dismiss={dismiss} />);
  }

  const aiValidateChangesEnabled = useFeatureFlagAsync(Internal.featureFlags?.AIValidateChanges);
  // For now, only support this in VS Code
  if (!commit && aiValidateChangesEnabled && platform.platformName === 'vscode') {
    actions.push(<ValidateChangesButton key="validate-changes" dismiss={dismiss} />);
  }

  const aiCodeReviewUpsellEnabled = useFeatureFlagAsync(Internal.featureFlags?.AICodeReviewUpsell);
  // For now, only support this in VS Code
  if (aiCodeReviewUpsellEnabled && platform.platformName === 'vscode') {
    const enabled = !commit || commit.isDot; // Enabled for `uncommitted changes` or the `current commit`.
    actions.push(
      <ReviewCodeButton
        key="review-commit"
        commit={commit}
        dismiss={dismiss}
        disabled={!enabled}
        disabledReason="This action is only available for the current commit and uncommitted changes."
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
      onSplitInitiated={() => {
        tracker.track('SmartActionClicked', {extras: {action: 'AutoSplit'}});
        dismiss();
      }}>
      <Icon icon="sparkle" />
      <T>Auto-split</T>
    </BaseSplitButton>
  );
}

/** Prompt AI to resolve all comments on a diff. */
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

/** Prompt AI to fill commit info. */
function FillCommitInfoButton({dismiss}: {dismiss: () => void}) {
  return (
    <Button
      data-testid="fill-commit-info-button"
      onClick={e => {
        tracker.track('SmartActionClicked', {extras: {action: 'FillCommitMessage'}});
        serverAPI.postMessage({
          type: 'platform/fillCommitMessageWithAI',
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

/** Prompt AI to resolve failed signals on a diff. */
function ResolveFailedSignalsButton({
  hash,
  diffId,
  dismiss,
  disabled,
  disabledReason,
}: {
  hash: string;
  diffId: string;
  dismiss: () => void;
  disabled?: boolean;
  disabledReason?: string;
}) {
  const repo = useAtomValue(repositoryInfo);
  const repoPath = repo?.repoRoot;
  const diffSummaryResult = useAtomValue(diffSummary(diffId));

  // Only show the button if there are failed signals
  if (
    diffSummaryResult.error ||
    !diffSummaryResult.value?.signalSummary ||
    diffSummaryResult.value.signalSummary !== 'failed'
  ) {
    return null;
  }

  const diffVersionNumber = Internal.getDiffVersionNumber?.(diffSummaryResult.value, hash);

  const button = (
    <Button
      data-testid="resolve-failed-signals-button"
      onClick={e => {
        if (diffVersionNumber !== undefined) {
          tracker.track('SmartActionClicked', {extras: {action: 'ResolveFailedSignals'}});
          serverAPI.postMessage({
            type: 'platform/resolveFailedSignalsWithAI',
            diffId,
            diffVersionNumber,
            repoPath,
          });
          dismiss();
        }
        e.stopPropagation();
      }}
      disabled={disabled || diffVersionNumber === undefined}>
      <Icon icon="sparkle" />
      <T>Fix failed signals</T>
    </Button>
  );

  return disabled || diffVersionNumber === undefined ? (
    <Tooltip
      title={
        diffVersionNumber === undefined
          ? 'Unable to determine Phabricator version number for this commit'
          : disabledReason
      }>
      {button}
    </Tooltip>
  ) : (
    button
  );
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
          type: 'platform/createTestForModifiedCodeWithAI',
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

/** Prompt AI to validate code and fix errors in the working copy. */
function ValidateChangesButton({dismiss}: {dismiss: () => void}) {
  return (
    <Button
      data-testid="validate-changes-button"
      onClick={e => {
        tracker.track('SmartActionClicked', {extras: {action: 'ValidateChanges'}});
        serverAPI.postMessage({
          type: 'platform/validateChangesWithAI',
        });
        dismiss();
        e.stopPropagation();
      }}>
      <Icon icon="sparkle" />
      <T>Validate Changes</T>
    </Button>
  );
}

/** Prompt AI to review the current commit and add comments */
function ReviewCodeButton({
  commit,
  dismiss,
  disabled,
  disabledReason,
}: {
  commit?: CommitInfo;
  dismiss: () => void;
  disabled?: boolean;
  disabledReason?: string;
}) {
  const button = (
    <Button
      data-testid="review-commit-button"
      onClick={e => {
        tracker.track('SmartActionClicked', {extras: {action: 'ReviewCommit'}});
        serverAPI.postMessage({
          type: 'platform/runAICodeReviewChat',
          source: 'smartAction',
          reviewScope: commit ? 'current commit' : 'uncommitted changes',
        });
        dismiss();
        e.stopPropagation();
      }}
      disabled={disabled}>
      <Icon icon="sparkle" />
      <T>{commit ? 'Review commit' : 'Review changes'}</T>
    </Button>
  );

  return disabled ? <Tooltip title={disabledReason}>{button}</Tooltip> : button;
}
