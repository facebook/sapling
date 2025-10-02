/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Banner, BannerKind} from 'isl-components/Banner';
import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {useAtom} from 'jotai';
import clientToServerAPI from '../ClientToServerAPI';
import {T} from '../i18n';
import {codeReviewStatusAtom} from './firstPassCodeReviewAtoms';

import {tracker} from '../analytics';
import {useFeatureFlagSync} from '../featureFlags';
import {Internal} from '../Internal';
import platform from '../platform';
import './AICodeReviewUpsell.css';

export function AICodeReviewUpsell(): JSX.Element | null {
  const [status, setStatus] = useAtom(codeReviewStatusAtom);
  const aiFirstPassCodeReviewEnabled = useFeatureFlagSync(
    Internal.featureFlags?.AIFirstPassCodeReview,
  );

  // TODO: move this component to vscode/webview
  if (platform.platformName !== 'vscode') {
    return null;
  }

  return (
    <Banner kind={BannerKind.default}>
      <div className="code-review-upsell-inner">
        <div className="code-review-upsell-icon-text">
          <Icon icon="sparkle" />
          {Internal.aiCodeReview
            ? `Get a code review from ${Internal.aiCodeReview.provider}`
            : 'Get an AI code review'}
        </div>
        <Button // TODO: Replace with dropdown to choose between quick/thorough review
          onClick={() => {
            setStatus('running');
            clientToServerAPI.postMessage({
              type: 'platform/runAICodeReviewChat',
              source: 'commitInfoView',
            });
            tracker.track('AICodeReviewInitiatedFromISL');
          }}
          disabled={aiFirstPassCodeReviewEnabled && status === 'running'}>
          {<T>Start review</T>}
        </Button>
      </div>
    </Banner>
  );
}
