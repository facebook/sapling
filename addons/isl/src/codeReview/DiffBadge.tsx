/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo, DiffId, DiffSummary} from '../types';
import type {UICodeReviewProvider} from './UICodeReviewProvider';
import type {ReactNode} from 'react';

import {useShowConfirmSubmitStack} from '../ConfirmSubmitStack';
import {ExternalLink} from '../ExternalLink';
import {Internal} from '../Internal';
import {Tooltip} from '../Tooltip';
import {T, t} from '../i18n';
import {CircleEllipsisIcon} from '../icons/CircleEllipsisIcon';
import {CircleExclamationIcon} from '../icons/CircleExclamationIcon';
import {PullRevOperation} from '../operations/PullRevOperation';
import {persistAtomToConfigEffect} from '../persistAtomToConfigEffect';
import platform from '../platform';
import {useRunOperation} from '../serverAPIState';
import {exactRevset} from '../types';
import {diffSummary, codeReviewProvider} from './CodeReviewInfo';
import {openerUrlForDiffUrl} from './github/GitHubUrlOpener';
import {SyncStatus, syncStatusAtom} from './syncStatus';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useState, Component, Suspense} from 'react';
import {atom, useRecoilValue} from 'recoil';
import {Icon} from 'shared/Icon';

import './DiffBadge.css';

export const showDiffNumberConfig = atom<boolean>({
  key: 'showDiffNumberConfig',
  default: false,
  effects: [persistAtomToConfigEffect('isl.show-diff-number')],
});

/**
 * Component that shows inline summary information about a Diff,
 * such as its status, number of comments, CI state, etc.
 */
export function DiffInfo({commit, hideActions}: {commit: CommitInfo; hideActions: boolean}) {
  const repo = useRecoilValue(codeReviewProvider);
  const diffId = commit.diffId;
  if (repo == null || diffId == null) {
    return null;
  }
  return (
    <DiffErrorBoundary provider={repo} diffId={diffId}>
      <Suspense fallback={<DiffSpinner diffId={diffId} provider={repo} />}>
        <DiffInfoInner commit={commit} diffId={diffId} provider={repo} hideActions={hideActions} />
      </Suspense>
    </DiffErrorBoundary>
  );
}

export function DiffBadge({
  diff,
  children,
  url,
  provider,
  syncStatus,
}: {
  diff?: DiffSummary;
  children?: ReactNode;
  url?: string;
  provider: UICodeReviewProvider;
  syncStatus?: SyncStatus;
}) {
  const openerUrl = useRecoilValue(openerUrlForDiffUrl(url));

  return (
    <ExternalLink href={openerUrl} className={`diff-badge ${provider.name}-diff-badge`}>
      <provider.DiffBadgeContent diff={diff} children={children} syncStatus={syncStatus} />
    </ExternalLink>
  );
}

function DiffSpinner({diffId, provider}: {diffId: DiffId; provider: UICodeReviewProvider}) {
  return (
    <span className="diff-spinner" data-testid="diff-spinner">
      <DiffBadge provider={provider}>
        <Icon icon="loading" />
      </DiffBadge>
      {provider.formatDiffNumber(diffId)}
    </span>
  );
}

function DiffInfoInner({
  diffId,
  commit,
  provider,
  hideActions,
}: {
  diffId: DiffId;
  commit: CommitInfo;
  provider: UICodeReviewProvider;
  hideActions: boolean;
}) {
  const diffInfoResult = useRecoilValue(diffSummary(diffId));
  const syncStatuses = useRecoilValue(syncStatusAtom);
  if (diffInfoResult.error) {
    return <DiffLoadError number={provider.formatDiffNumber(diffId)} provider={provider} />;
  }
  if (diffInfoResult?.value == null) {
    return <DiffSpinner diffId={diffId} provider={provider} />;
  }
  const info = diffInfoResult.value;
  const syncStatus = syncStatuses?.get(commit.hash);
  return (
    <div
      className={`diff-info ${provider.name}-diff-info`}
      data-testid={`${provider.name}-diff-info`}>
      <DiffSignalSummary diff={info} />
      <DiffBadge provider={provider} diff={info} url={info.url} syncStatus={syncStatus} />
      {provider.DiffLandButtonContent && (
        <provider.DiffLandButtonContent diff={info} commit={commit} />
      )}
      <DiffComments diff={info} />
      <DiffNumber>{provider.formatDiffNumber(diffId)}</DiffNumber>
      {hideActions === true ? null : syncStatus === SyncStatus.RemoteIsNewer ? (
        <DownloadNewVersionButton diffId={diffId} provider={provider} />
      ) : syncStatus === SyncStatus.LocalIsNewer ? (
        <ResubmitSyncButton commit={commit} provider={provider} />
      ) : null}
    </div>
  );
}

function DownloadNewVersionButton({
  diffId,
  provider,
}: {
  diffId: DiffId;
  provider: UICodeReviewProvider;
}) {
  const runOperation = useRunOperation();
  return (
    <Tooltip
      title={t('$provider has a newer version of this Diff. Click to download the newer version.', {
        replace: {$provider: provider.label},
      })}>
      <VSCodeButton
        appearance="icon"
        onClick={() => {
          if (Internal.diffDownloadOperation != null) {
            runOperation(Internal.diffDownloadOperation(exactRevset(diffId)));
          } else {
            runOperation(new PullRevOperation(exactRevset(diffId)));
          }
        }}>
        <Icon icon="cloud-download" slot="start" />
        <T>Download New Version</T>
      </VSCodeButton>
    </Tooltip>
  );
}

function ResubmitSyncButton({
  commit,
  provider,
}: {
  commit: CommitInfo;
  provider: UICodeReviewProvider;
}) {
  const runOperation = useRunOperation();
  const confirmShouldSubmit = useShowConfirmSubmitStack();

  return (
    <Tooltip
      title={t('This commit has changed locally since it was last submitted. Click to resubmit.')}>
      <VSCodeButton
        appearance="icon"
        data-testid="commit-submit-button"
        onClick={async () => {
          const confirmation = await confirmShouldSubmit('submit', [commit]);
          if (!confirmation) {
            return [];
          }
          runOperation(
            provider.submitOperation([commit], {
              draft: confirmation.submitAsDraft,
              updateMessage: confirmation.updateMessage,
            }),
          );
        }}>
        <Icon icon="cloud-upload" slot="start" />
        <T>Submit</T>
      </VSCodeButton>
    </Tooltip>
  );
}

function DiffNumber({children}: {children: string}) {
  const [showing, setShowing] = useState(false);
  const showDiffNumber = useRecoilValue(showDiffNumberConfig);
  if (!children || !showDiffNumber) {
    return null;
  }

  return (
    <Tooltip trigger="manual" shouldShow={showing} title={t(`Copied ${children} to the clipboard`)}>
      <span
        className="diff-number"
        onClick={() => {
          platform.clipboardCopy(children);
          setShowing(true);
          setTimeout(() => setShowing(false), 2000);
        }}>
        {children}
      </span>
    </Tooltip>
  );
}

function DiffComments({diff}: {diff: DiffSummary}) {
  if (!diff.commentCount) {
    return null;
  }
  return (
    <div className="diff-comments-count">
      {diff.commentCount}
      <Icon icon={diff.anyUnresolvedComments ? 'comment-unresolved' : 'comment'} />
    </div>
  );
}

function DiffSignalSummary({diff}: {diff: DiffSummary}) {
  if (!diff.signalSummary) {
    return null;
  }
  let icon;
  let tooltip;
  switch (diff.signalSummary) {
    case 'running':
      icon = <CircleEllipsisIcon />;
      tooltip = t('Test Signals are still running for this Diff.');
      break;
    case 'pass':
      icon = 'check';
      tooltip = t('Test Signals completed successfully for this Diff.');
      break;
    case 'failed':
      icon = 'error';
      tooltip = t(
        'An error was encountered during the test signals on this Diff. See Diff for more details.',
      );
      break;
    case 'no-signal':
      icon = 'question';
      tooltip = t('No signal from test run on this Diff.');
      break;
    case 'warning':
      icon = <CircleExclamationIcon />;
      tooltip = t(
        'Test Signals were not fully successful for this Diff. See Diff for more details.',
      );
      break;
  }
  return (
    <div className={`diff-signal-summary diff-signal-${diff.signalSummary}`}>
      <Tooltip title={tooltip}>{typeof icon === 'string' ? <Icon icon={icon} /> : icon}</Tooltip>
    </div>
  );
}

export class DiffErrorBoundary extends Component<
  {
    children: React.ReactNode;
    diffId: string;
    provider: UICodeReviewProvider;
  },
  {error: Error | null}
> {
  state = {error: null};
  static getDerivedStateFromError(error: Error) {
    return {error};
  }
  render() {
    if (this.state.error != null) {
      return (
        <DiffLoadError
          provider={this.props.provider}
          number={this.props.provider.formatDiffNumber(this.props.diffId)}
        />
      );
    }
    return this.props.children;
  }
}

function DiffLoadError({number, provider}: {number: string; provider: UICodeReviewProvider}) {
  return (
    <span className="diff-error diff-info" data-testid={`${provider.name}-error`}>
      <DiffBadge provider={provider}>
        <Icon icon="error" />
      </DiffBadge>{' '}
      {number}
    </span>
  );
}
