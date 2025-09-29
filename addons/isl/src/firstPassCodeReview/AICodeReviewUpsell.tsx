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
import {serverCwd} from '../repositoryData';
import {codeReviewStatusAtom} from './firstPassCodeReviewAtoms';

import {tracker} from '../analytics';
import './AICodeReviewUpsell.css';

export function AICodeReviewUpsell(): JSX.Element {
  const cwd = useAtomValue(serverCwd); // TODO: Remove this once we are running through DVSC
  const [status, setStatus] = useAtom(codeReviewStatusAtom);

  return (
    <Banner kind={BannerKind.default}>
      <div className="code-review-upsell-inner">
        <div className="code-review-upsell-icon-text">
          <Icon icon="info" color="blue" />
          Get a code review from Devmate
        </div>
        <Button // TODO: Replace with dropdown to choose between quick/thorough review
          onClick={() => {
            setStatus('running');
            clientToServerAPI.postMessage({
              type: 'platform/runAICodeReview',
              cwd,
            });
            tracker.track('AICodeReviewInitiatedFromISL');
          }}
          disabled={status === 'running'}>
          {<T>Start review</T>}
        </Button>
      </div>
    </Banner>
  );
}
