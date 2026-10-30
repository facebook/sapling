/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Banner, BannerKind} from 'isl-components/Banner';
import {Button} from 'isl-components/Button';
import {ButtonDropdown} from 'isl-components/ButtonDropdown';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtom, useAtomValue, useSetAtom} from 'jotai';
import clientToServerAPI from '../ClientToServerAPI';
import {latestHeadCommit} from '../serverAPIState';
import type {CodeReviewScope} from '../types';
import {codeReviewStatusAtom, lastSelectedReviewOptionIdAtom} from './firstPassCodeReviewAtoms';

import type {JSX} from 'react';
import {useCallback, useEffect, useState} from 'react';
import {tracker} from '../analytics';
import {useFeatureFlagSync} from '../featureFlags';
import {Internal} from '../Internal';
import platform from '../platform';
import './AICodeReviewUpsell.css';

export function AICodeReviewUpsell({
  isCommitMode,
  hasUncommittedChanges,
}: {
  isCommitMode: boolean;
  hasUncommittedChanges: boolean;
}): JSX.Element | null {
  const [status, setStatus] = useAtom(codeReviewStatusAtom);
  const headCommit = useAtomValue(latestHeadCommit);
  const [hidden, setHidden] = useState(false);
  const aiFirstPassCodeReviewEnabled = useFeatureFlagSync(
    Internal.featureFlags?.AIFirstPassCodeReview,
  );

  const reviewOptions: Array<{
    label: string;
    id: string;
  }> = Internal.aiCodeReview?.reviewOptions ?? [];

  const reviewScope: CodeReviewScope = isCommitMode
    ? 'uncommitted changes'
    : hasUncommittedChanges
      ? 'current commit and uncommitted changes'
      : 'current commit';

  const startReview = useCallback(
    (agentBackend: string) => {
      setStatus('running');
      setHidden(true);
      clientToServerAPI.postMessage({
        type: 'platform/runAICodeReviewChat',
        reviewScope,
        source: 'commitInfoView',
        agentBackend: agentBackend as 'devmate' | 'claude',
      });
      tracker.track('AICodeReviewInitiatedFromISL', {
        extras: {agentBackend},
      });
    },
    [setStatus, reviewScope],
  );

  const lastSelectedId = useAtomValue(lastSelectedReviewOptionIdAtom);
  const setLastSelectedId = useSetAtom(lastSelectedReviewOptionIdAtom);
  const selectedOption = reviewOptions.find(opt => opt.id === lastSelectedId) ?? reviewOptions[0];

  useEffect(() => {
    setHidden(false);
  }, [headCommit]);

  // TODO: move this component to vscode/webview
  if (platform.platformName !== 'vscode') {
    return null;
  }

  if (hidden || reviewOptions.length === 0) {
    return null;
  }

  const bannerText = 'Get AI feedback before submitting';

  const isReviewInProgress = aiFirstPassCodeReviewEnabled && status === 'running';
  const noUncommittedChanges = isCommitMode && !hasUncommittedChanges;
  const shouldDisableButton = isReviewInProgress || noUncommittedChanges;
  const disabledReason = isReviewInProgress
    ? 'Review already in progress'
    : noUncommittedChanges
      ? 'No uncommitted changes'
      : null;

  const reviewButton =
    reviewOptions.length === 1 ? (
      shouldDisableButton && disabledReason ? (
        <Tooltip title={disabledReason}>
          <Button data-testid="start-review-button" disabled={true}>
            Start review
          </Button>
        </Tooltip>
      ) : (
        <Button
          data-testid="start-review-button"
          disabled={shouldDisableButton}
          onClick={() => startReview(reviewOptions[0].id)}>
          Start review
        </Button>
      )
    ) : (
      <ButtonDropdown
        data-testid="start-review-button"
        options={reviewOptions}
        selected={selectedOption}
        onChangeSelected={option => {
          setLastSelectedId(option.id);
          startReview(option.id);
        }}
        onClick={selected => startReview(selected.id)}
        buttonDisabled={shouldDisableButton}
        pickerDisabled={shouldDisableButton}
        primaryTooltip={shouldDisableButton && disabledReason ? {title: disabledReason} : undefined}
      />
    );

  return (
    <Banner kind={BannerKind.default} buttons={reviewButton} alwaysShowButtons>
      <div className="code-review-upsell-inner">
        <div className="code-review-upsell-icon-text">
          <Icon icon="sparkle" />
          {bannerText}
        </div>
      </div>
    </Banner>
  );
}
