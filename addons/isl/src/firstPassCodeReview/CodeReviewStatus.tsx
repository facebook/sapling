/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Banner, BannerKind} from 'isl-components/Banner';
import {Button} from 'isl-components/Button';
import {T} from '../i18n';
import {runCodeReview} from './runCodeReview';
import {useAtomValue} from 'jotai';
import {serverCwd} from '../repositoryData';
import clientToServerAPI from '../ClientToServerAPI';
import {useState} from 'react';
import {Icon} from 'isl-components/Icon';
import type {CommitInfo} from '../types';

import './CodeReviewStatus.css';
import {Tooltip} from 'isl-components/Tooltip';

type CodeReviewProgressStatus = 'running' | 'success' | 'error';

export function CodeReviewStatus({commit}: {commit: CommitInfo}): JSX.Element {
  const cwd = useAtomValue(serverCwd);
  const [status, setStatus] = useState<CodeReviewProgressStatus | null>(null);

  const button = (
    <Button
      onClick={async () => {
        let results;
        setStatus('running');
        try {
          results = await runCodeReview(cwd);
        } catch (e) {
          setStatus('error');
          return;
        }
        clientToServerAPI.postMessage({
          type: 'platform/setFirstPassCodeReviewDiagnostics',
          issueMap: results,
        });
        setStatus('success');
      }}
      disabled={!commit.isDot}>
      {status == null ? <T>Try it!</T> : <T>Try again</T>}
    </Button>
  );

  return (
    <Banner kind={getBannerKind(status)}>
      <div className="code-review-status-inner">
        <b>
          <BannerText status={status} />
        </b>
        {status === 'running' ? (
          <Icon icon="loading" />
        ) : commit.isDot ? (
          button
        ) : (
          <Tooltip title="This action is only available for the current commit.">{button}</Tooltip>
        )}
      </div>
    </Banner>
  );
}

function BannerText({status}: {status: CodeReviewProgressStatus | null}) {
  switch (status) {
    case 'running':
      return <T>Running code review...</T>;
    case 'success':
      return <T>Code review complete!</T>;
    case 'error':
      return <T>Code review failed.</T>;
    default:
      return <T>Review your code using Devmate.</T>;
  }
}

function getBannerKind(status: CodeReviewProgressStatus | null) {
  switch (status) {
    case 'running':
      return BannerKind.default;
    case 'success':
      return BannerKind.green;
    case 'error':
      return BannerKind.error;
    default:
      return BannerKind.default;
  }
}
