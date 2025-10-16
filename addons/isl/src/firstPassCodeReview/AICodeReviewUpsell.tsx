/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Banner, BannerKind} from 'isl-components/Banner';
import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {useAtom, useAtomValue} from 'jotai';
import clientToServerAPI from '../ClientToServerAPI';
import {T} from '../i18n';
import {latestHeadCommit} from '../serverAPIState';
import {codeReviewStatusAtom} from './firstPassCodeReviewAtoms';

import {Tooltip} from 'isl-components/Tooltip';
import {useEffect, useState} from 'react';
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

  useEffect(() => {
    setHidden(false);
  }, [headCommit]);

  // TODO: move this component to vscode/webview
  if (platform.platformName !== 'vscode') {
    return null;
  }

  if (hidden) {
    return null;
  }

  const codeReviewProvider = Internal.aiCodeReview?.provider ?? 'AI';
  const bannerText = isCommitMode
    ? `Use ${codeReviewProvider} to review your uncommitted changes`
    : hasUncommittedChanges
      ? `Use ${codeReviewProvider} to review this commit and your uncommitted changes`
      : `Use ${codeReviewProvider} to review this commit`;

  const isReviewInProgress = aiFirstPassCodeReviewEnabled && status === 'running';
  const noUncommittedChanges = isCommitMode && !hasUncommittedChanges;
  const shouldDisableButton = isReviewInProgress || noUncommittedChanges;
  const disabledReason = isReviewInProgress
    ? 'Review already in progress'
    : noUncommittedChanges
      ? 'No uncommitted changes'
      : null;

  const button = (
    <Button // TODO: Replace with dropdown to choose between quick/thorough review
      onClick={() => {
        setStatus('running');
        setHidden(true);
        clientToServerAPI.postMessage({
          type: 'platform/runAICodeReviewChat',
          reviewScope: isCommitMode
            ? 'uncommitted changes'
            : hasUncommittedChanges
              ? 'current commit and uncommitted changes'
              : 'current commit',
          source: 'commitInfoView',
        });
        tracker.track('AICodeReviewInitiatedFromISL');
      }}
      disabled={shouldDisableButton}>
      {<T>Start review</T>}
    </Button>
  );

  return (
    <Banner kind={BannerKind.default}>
      <div className="code-review-upsell-inner">
        <div className="code-review-upsell-icon-text">
          <Icon icon="sparkle" />
          {bannerText}
        </div>
        {shouldDisableButton && disabledReason ? (
          <Tooltip title={disabledReason}>{button}</Tooltip>
        ) : (
          button
        )}
      </div>
    </Banner>
  );
}
