/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Banner} from 'isl-components/Banner';
import {Button} from 'isl-components/Button';
import {T} from '../i18n';
import {runCodeReview} from './runCodeReview';
import {useAtomValue} from 'jotai';
import {serverCwd} from '../repositoryData';

import './CodeReviewStatus.css';
import clientToServerAPI from '../ClientToServerAPI';

export function CodeReviewStatus(): JSX.Element {
  const cwd = useAtomValue(serverCwd);

  return (
    <Banner>
      <div className="code-review-status-inner">
        <b>
          <T>Review your code using Devmate.</T>
        </b>
        <Button
          onClick={async () => {
            let results;
            try {
              results = await runCodeReview(cwd);
            } catch (e) {
              // eslint-disable-next-line no-console
              console.error(e);
              return;
            }
            clientToServerAPI.postMessage({
              type: 'platform/setFirstPassCodeReviewDiagnostics',
              issueMap: results,
            });
          }}>
          <T>Try it!</T>
        </Button>
      </div>
    </Banner>
  );
}
