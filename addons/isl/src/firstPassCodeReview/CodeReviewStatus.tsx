/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Banner, BannerKind} from 'isl-components/Banner';
import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {atom, useAtom, useAtomValue} from 'jotai';
import clientToServerAPI from '../ClientToServerAPI';
import {T} from '../i18n';
import {writeAtom} from '../jotaiUtils';
import {serverCwd} from '../repositoryData';
import type {CommitInfo} from '../types';

import {registerDisposable} from '../utils';
import './CodeReviewStatus.css';
import {firstPassCommentData} from './firstPassCodeReviewAtoms';

type CodeReviewProgressStatus = 'running' | 'success' | 'error';

/**
 * Atom family to store code review status per commit hash.
 * Each commit gets its own atom to track its code review progress.
 */
const codeReviewStatusAtom = atom<CodeReviewProgressStatus | null>(null);

registerDisposable(
  firstPassCommentData,
  clientToServerAPI.onMessageOfType('platform/gotAIReviewComments', data => {
    const result = data.comments;
    if (result.error) {
      writeAtom(codeReviewStatusAtom, 'error');
    }
    writeAtom(codeReviewStatusAtom, 'success');
  }),
  import.meta.hot,
);

export function CodeReviewStatus({commit}: {commit: CommitInfo}): JSX.Element {
  const cwd = useAtomValue(serverCwd);
  const [status, setStatus] = useAtom(codeReviewStatusAtom);

  const button = (
    <Button
      onClick={() => {
        setStatus('running');
        clientToServerAPI.postMessage({
          type: 'platform/runAICodeReview',
          cwd,
        });
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
