/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';
import type {CommitInfo, DiffId, DiffSummary} from '../types';
import type {UICodeReviewProvider} from './UICodeReviewProvider';

import * as stylex from '@stylexjs/stylex';
import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtomValue} from 'jotai';
import {Component, lazy, Suspense, useState} from 'react';
import {useShowConfirmSubmitStack} from '../ConfirmSubmitStack';
import {Internal} from '../Internal';
import {Link} from '../Link';
import {clipboardCopyLink, clipboardCopyText} from '../clipboard';
import {T, t} from '../i18n';
import {CircleEllipsisIcon} from '../icons/CircleEllipsisIcon';
import {CircleExclamationIcon} from '../icons/CircleExclamationIcon';
import {configBackedAtom, useAtomGet} from '../jotaiUtils';
import {PullRevOperation} from '../operations/PullRevOperation';
import {useRunOperation} from '../operationsState';
import platform from '../platform';
import {exactRevset} from '../types';
import {codeReviewProvider, diffSummary} from './CodeReviewInfo';
import {openerUrlForDiffUrl} from './github/GitHubUrlOpener';
import {SyncStatus, syncStatusAtom} from './syncStatus';

import './DiffBadge.css';

const DiffCommentsDetails = lazy(() => import('./DiffComments'));

export const showDiffNumberConfig = configBackedAtom<boolean>('isl.show-diff-number', false);

/**
 * Component that shows inline summary information about a Diff,
 * such as its status, number of comments, CI state, etc.
 */
export function DiffInfo({commit, hideActions}: {commit: CommitInfo; hideActions: boolean}) {
  const repo = useAtomValue(codeReviewProvider);
  const {diffId} = commit;
  if (repo == null || diffId == null) {
    return null;
  }

  // Do not show diff info (and "Ship It" button) if there are successors.
  // Users should look at the diff info and buttons from the successor commit instead.
  // But the diff number can still be useful so show it.
  if (commit.successorInfo != null) {
    return <DiffNumber>{repo.formatDiffNumber(diffId)}</DiffNumber>;
  }
  return (
    <DiffErrorBoundary provider={repo} diffId={diffId}>
      <Suspense fallback={<DiffSpinner diffId={diffId} provider={repo} />}>
        <DiffInfoInner commit={commit} diffId={diffId} provider={repo} hideActions={hideActions} />
      </Suspense>
    </DiffErrorBoundary>
  );
}

const styles = stylex.create({
  diffBadge: {
    color: 'white',
    cursor: 'pointer',
    textDecoration: {
      default: 'none',
      ':hover': 'underline',
    },
  },
  diffFollower: {
    alignItems: 'center',
    display: 'inline-flex',
    gap: '5px',
    opacity: '0.9',
    fontSize: '90%',
    padding: '0 var(--halfpad)',
  },
  diffFollowerIcon: {
    '::before': {
      fontSize: '90%',
    },
  },
});

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
  const openerUrl = useAtomValue(openerUrlForDiffUrl(url));

  return (
    <Link href={openerUrl} xstyle={styles.diffBadge}>
      <provider.DiffBadgeContent diff={diff} children={children} syncStatus={syncStatus} />
    </Link>
  );
}

export function DiffFollower({commit}: {commit: CommitInfo}) {
  if (!commit.isFollower) {
    return null;
  }

  return (
    <Tooltip title={t('This commit follows the Pull Request of its nearest descendant above')}>
      <span {...stylex.props(styles.diffFollower)}>
        <Icon icon="fold-up" size="S" {...stylex.props(styles.diffFollowerIcon)} />
        <T>follower</T>
      </span>
    </Tooltip>
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
  const diffInfoResult = useAtomValue(diffSummary(diffId));
  const syncStatus = useAtomGet(syncStatusAtom, commit.hash);
  if (diffInfoResult.error) {
    return <DiffLoadError number={provider.formatDiffNumber(diffId)} provider={provider} />;
  }
  if (diffInfoResult?.value == null) {
    return <DiffSpinner diffId={diffId} provider={provider} />;
  }
  const info = diffInfoResult.value;
  const shouldHideActions = hideActions || provider.isDiffClosed(info);
  return (
    <div
      className={`diff-info ${provider.name}-diff-info`}
      data-testid={`${provider.name}-diff-info`}>
      <DiffSignalSummary diff={info} />
      <DiffBadge provider={provider} diff={info} url={info.url} syncStatus={syncStatus} />
      {provider.DiffLandButtonContent && (
        <provider.DiffLandButtonContent diff={info} commit={commit} />
      )}
      <DiffComments diffId={diffId} diff={info} />
      <DiffNumber url={info.url}>{provider.formatDiffNumber(diffId)}</DiffNumber>
      {shouldHideActions ? null : syncStatus === SyncStatus.RemoteIsNewer ? (
        <DownloadNewVersionButton diffId={diffId} provider={provider} />
      ) : syncStatus === SyncStatus.BothChanged ? (
        <DownloadNewVersionButton diffId={diffId} provider={provider} bothChanged />
      ) : syncStatus === SyncStatus.LocalIsNewer ? (
        <ResubmitSyncButton commit={commit} provider={provider} />
      ) : null}
    </div>
  );
}

function DownloadNewVersionButton({
  diffId,
  provider,
  bothChanged,
}: {
  diffId: DiffId;
  provider: UICodeReviewProvider;
  bothChanged?: boolean;
}) {
  const runOperation = useRunOperation();
  const tooltip = bothChanged
    ? t(
        'Both remote and local versions have changed.\n\n$provider has a new version of this Diff, but this commit has also changed locally since it was last submitted. You can download the new remote version, but it may not include your other local changes.',
        {replace: {$provider: provider.label}},
      )
    : t('$provider has a newer version of this Diff. Click to download the newer version.', {
        replace: {$provider: provider.label},
      });

  return (
    <Tooltip title={tooltip}>
      <Button
        icon
        onClick={async () => {
          if (bothChanged) {
            const confirmed = await platform.confirm(tooltip);
            if (confirmed !== true) {
              return;
            }
          }
          if (Internal.diffDownloadOperation != null) {
            runOperation(Internal.diffDownloadOperation(exactRevset(diffId)));
          } else {
            runOperation(new PullRevOperation(exactRevset(diffId)));
          }
        }}>
        <Icon icon="cloud-download" slot="start" />
        <T>Download New Version</T>
      </Button>
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
      <Button
        icon
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
      </Button>
    </Tooltip>
  );
}

function DiffNumber({children, url}: {children: string; url?: string}) {
  const [showing, setShowing] = useState(false);
  const showDiffNumber = useAtomValue(showDiffNumberConfig);
  if (!children || !showDiffNumber) {
    return null;
  }

  return (
    <Tooltip trigger="manual" shouldShow={showing} title={t(`Copied ${children} to the clipboard`)}>
      <span
        className="diff-number"
        onClick={() => {
          url == null ? clipboardCopyText(children) : clipboardCopyLink(children, url);
          setShowing(true);
          setTimeout(() => setShowing(false), 2000);
        }}>
        {children}
      </span>
    </Tooltip>
  );
}

function DiffComments({diff, diffId}: {diff: DiffSummary; diffId: DiffId}) {
  if (!diff.commentCount) {
    return null;
  }
  return (
    <Tooltip
      trigger="click"
      component={() => (
        <Suspense>
          <DiffCommentsDetails diffId={diffId} />
        </Suspense>
      )}>
      <Button icon>
        <span className="diff-comments-count">
          {diff.commentCount}
          <Icon icon={diff.anyUnresolvedComments ? 'comment-unresolved' : 'comment'} />
        </span>
      </Button>
    </Tooltip>
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
    case 'land-cancelled':
      icon = <CircleExclamationIcon />;
      tooltip = t('Land is cancelled for this Diff. See Diff for more details.');
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
