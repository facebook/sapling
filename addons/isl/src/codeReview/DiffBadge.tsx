/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';
import type {CommitInfo, DiffId, DiffSummary} from '../types';
import type {UICodeReviewProvider} from './UICodeReviewProvider';

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtomValue} from 'jotai';
import {Component, lazy, Suspense, useRef, useState} from 'react';
import {useShowConfirmSubmitStack} from '../ConfirmSubmitStack';
import {Internal} from '../Internal';
import {Link} from '../Link';
import {clipboardCopyLink, clipboardCopyText} from '../clipboard';
import {useFeatureFlagSync} from '../featureFlags';
import {T, t} from '../i18n';
import {CircleExclamationIcon} from '../icons/CircleExclamationIcon';
import {IconStack} from '../icons/IconStack';
import {atomFamilyWeak, atomLoadableWithRefresh, configBackedAtom, useAtomGet} from '../jotaiUtils';
import {PullRevOperation} from '../operations/PullRevOperation';
import {useRunOperation} from '../operationsState';
import platform from '../platform';
import {inMergeConflicts, repositoryInfo} from '../serverAPIState';
import {exactRevset} from '../types';
import {codeReviewProvider, diffSummary} from './CodeReviewInfo';
import './DiffBadge.css';
import css from './DiffBadge.module.css';
import {submitAsDraft} from './DraftCheckbox';
import {openerUrlForDiffUrl} from './github/GitHubUrlOpener';
import {SyncStatus, syncStatusAtom} from './syncStatus';

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
    <Link href={openerUrl} className={css.diffBadge}>
      <provider.DiffBadgeContent diff={diff} syncStatus={syncStatus}>
        {children}
      </provider.DiffBadgeContent>
    </Link>
  );
}

export function DiffFollower({commit}: {commit: CommitInfo}) {
  if (!commit.isFollower) {
    return null;
  }

  return (
    <Tooltip title={t('This commit follows the Pull Request of its nearest descendant above')}>
      <span className={css.diffFollower}>
        <Icon icon="fold-up" size="S" className={css.diffFollowerIcon} />
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
  const startTestsEnabled = useFeatureFlagSync(Internal.featureFlags?.StartTestsButton);
  const isInMergeConflicts = useAtomValue(inMergeConflicts);
  if (diffInfoResult.error) {
    return <DiffLoadError number={provider.formatDiffNumber(diffId)} provider={provider} />;
  }
  if (diffInfoResult?.value == null) {
    return <DiffSpinner diffId={diffId} provider={provider} />;
  }
  const info = diffInfoResult.value;
  const shouldHideActions = hideActions || provider.isDiffClosed(info);
  // deferredTestingInfo is fb-only (phabricator). Use 'in' check to avoid OSS type errors.
  const deferredTestingInfo:
    | {
        submitQueueRequestFBID?: string | null;
        explanation?: string | null;
        isDeferred?: boolean;
      }
    | undefined =
    'deferredTestingInfo' in info
      ? (info.deferredTestingInfo as {
          submitQueueRequestFBID?: string | null;
          explanation?: string | null;
          isDeferred?: boolean;
        })
      : undefined;
  // Use version-level isDeferred from deferredTestingInfo for accurate detection
  const isDeferred = deferredTestingInfo?.isDeferred === true || info.signalSummary === 'deferred';

  return (
    <div
      className={`diff-info ${provider.name}-diff-info`}
      data-testid={`${provider.name}-diff-info`}>
      <DiffSignalSummary commit={commit} diff={info} diffId={diffId} />
      <DiffBadge provider={provider} diff={info} url={info.url} syncStatus={syncStatus} />
      {provider.DiffLandButtonContent && !isInMergeConflicts && (
        <provider.DiffLandButtonContent diff={info} commit={commit} />
      )}
      {/* Show Start Tests button when deferred (fb-only) */}
      {startTestsEnabled &&
        isDeferred &&
        Internal.StartDeferredTestsButton != null &&
        deferredTestingInfo?.submitQueueRequestFBID && (
          <Suspense fallback={null}>
            <Internal.StartDeferredTestsButton
              diffId={diffId}
              submitQueueRequestFBID={deferredTestingInfo.submitQueueRequestFBID}
              explanation={deferredTestingInfo?.explanation}
            />
          </Suspense>
        )}
      <DiffComments diffId={diffId} diff={info} />
      <DiffNumber url={info.url} versionLabel={Internal.getDiffVersionLabel?.(info, commit.hash)}>
        {provider.formatDiffNumber(diffId)}
      </DiffNumber>
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
  const shouldSubmitAsDraft = useAtomValue(submitAsDraft);

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
              publishWhenReady: confirmation.publishWhenReady,
            }),
          );
        }}>
        <Icon icon="cloud-upload" slot="start" />
        {shouldSubmitAsDraft ? <T>Submit Draft</T> : <T>Submit</T>}
      </Button>
    </Tooltip>
  );
}

function DiffNumber({
  children,
  url,
  versionLabel,
}: {
  children: string;
  url?: string;
  versionLabel?: string;
}) {
  const [showing, setShowing] = useState(false);
  const showDiffNumber = useAtomValue(showDiffNumberConfig);
  if (!children || !showDiffNumber) {
    return null;
  }

  return (
    <Tooltip trigger="manual" shouldShow={showing} title={t(`Copied ${children} to the clipboard`)}>
      <span
        className="diff-number"
        onClick={e => {
          url == null ? clipboardCopyText(children) : clipboardCopyLink(children, url);
          setShowing(true);
          setTimeout(() => setShowing(false), 2000);
          e.stopPropagation();
        }}>
        {children}
        {versionLabel != null && <span className="diff-version-label"> {versionLabel}</span>}
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

/** A null key resolves to null without fetching, so ineligible diffs cost no requests. */
const diffSignalCountFamily = atomFamilyWeak((diffId: DiffId | null) =>
  atomLoadableWithRefresh(async get => {
    const {fetchDiffSignalCount} = Internal;
    if (diffId == null || fetchDiffSignalCount == null) {
      return null;
    }
    // Read before awaiting so the dependency registers: every summary fetch produces a new summary
    // object, which re-runs this and re-asks for the count. Counts move as CI advances, so without
    // a dependency the first answer would be the only one this window ever shows. Re-asking is
    // cheap because the server caches and coalesces, and a forced refresh drops that cache when it
    // kicks off the refetch.
    get(diffSummary(diffId));
    const count = await fetchDiffSignalCount(diffId);
    return count;
  }),
);

function DiffSignalSummary({
  commit,
  diff,
  diffId,
}: {
  commit: CommitInfo;
  diff: DiffSummary;
  diffId?: DiffId;
}) {
  const repo = useAtomValue(repositoryInfo);

  // Only Phabricator answers this ask, and nothing on the client times out one that never gets a
  // reply — but GitHub summaries carry a signal summary too, so the provider has to be part of the
  // condition. Every state but 'no-signal' and 'deferred' is worth asking about: even a 'pass' diff
  // can have INFO signals we want to count.
  const shouldFetchCount =
    repo?.codeReviewSystem.type === 'phabricator' &&
    diffId != null &&
    diff.signalSummary != null &&
    diff.signalSummary !== 'no-signal' &&
    diff.signalSummary !== 'deferred';

  // Eligibility is part of the key, so flipping it re-keys the atom and refetches on its own.
  const countLoadable = useAtomValue(diffSignalCountFamily(shouldFetchCount ? diffId : null));

  // Each re-run produces a promise `loadable` has not seen, which it reports as `loading` with no
  // memory of the last value. Rendering that as "no count" would unmount the digit on every summary
  // push, so show the previous answer until the new one lands. Keyed so a reused component
  // instance cannot inherit another diff's count.
  const countKey = shouldFetchCount ? diffId : null;
  const lastCount = useRef<{key: DiffId | null | undefined; count: number | null}>({
    key: undefined,
    count: null,
  });
  if (countLoadable.state === 'hasData') {
    lastCount.current = {key: countKey, count: countLoadable.data};
  } else if (countLoadable.state === 'hasError') {
    // A rejected ask carries no answer, so holding the previous digit would keep asserting a count
    // nothing stands behind. Today the server turns every failure into `count: null`, so this is
    // only reachable if that ever stops being true.
    lastCount.current = {key: countKey, count: null};
  }
  const signalCount =
    countLoadable.state === 'hasData'
      ? countLoadable.data
      : lastCount.current.key === countKey
        ? lastCount.current.count
        : null;

  if (!diff.signalSummary) {
    return null;
  }
  let icon;
  let tooltip;
  switch (diff.signalSummary) {
    case 'running':
      icon = <Icon icon="sync" />;
      tooltip = t('Test Signals are still running for this Diff.');
      break;
    case 'running-warnings':
      icon = (
        <IconStack>
          <Icon icon="sync" />
          <Icon icon="circle-filled" color="yellow" />
        </IconStack>
      );
      tooltip = t(
        'Test Signals are still running for this Diff, with warnings so far. Click for more details.',
      );
      break;
    case 'running-failed':
      icon = (
        <IconStack>
          <Icon icon="sync" />
          <Icon icon="circle-filled" color="red" />
        </IconStack>
      );
      tooltip = t(
        'Test Signals are still running for this Diff, with failures so far. Click for more details.',
      );
      break;
    case 'pass':
      icon = 'pass';
      tooltip = t('Test Signals completed successfully for this Diff.');
      break;
    case 'failed':
      icon = 'error';
      tooltip = t(
        'An error was encountered during the test signals on this Diff. Click for more details.',
      );
      break;
    case 'no-signal':
      icon = 'question';
      tooltip = t('No signal from test run on this Diff.');
      break;
    case 'warning':
      icon = <CircleExclamationIcon />;
      tooltip = t('Test Signals were not fully successful for this Diff. Click for more details.');
      break;
    case 'land-cancelled':
      icon = 'circle-slash';
      tooltip = t('Land was cancelled for this Diff. Click for more details.');
      break;
    case 'land-on-hold':
      icon = 'debug-pause';
      tooltip = t('Land is on hold for this Diff. See Diff for more details.');
      break;
    case 'deferred':
      icon = 'debug-pause';
      tooltip = t('Tests are deferred for this Diff. Click "Start Tests" to run them.');
      break;
  }

  const renderedIcon = typeof icon === 'string' ? <Icon icon={icon} /> : icon;

  if (diffId != null && Internal.DiffSignalDetailsComponent != null) {
    const DiffSignalDetailsComponent = Internal.DiffSignalDetailsComponent;
    // Get diffVersionNumber from diff if available (for phabricator diffs)
    const diffVersionNumber = Internal.getDiffVersionNumber?.(diff, commit.hash);
    return (
      <Tooltip
        trigger="click"
        title={tooltip}
        component={() => (
          <Suspense fallback={<Icon icon="loading" />}>
            <DiffSignalDetailsComponent
              commit={commit}
              diffId={diffId}
              diffVersionNumber={diffVersionNumber}
              repoPath={repo?.repoRoot}
            />
          </Suspense>
        )}>
        <Button icon>
          <span className="diff-signals-button">
            {signalCount != null && signalCount > 0 && (
              <span className="diff-signals-count">{signalCount}</span>
            )}
            <span className={`diff-signals-icon diff-signal-${diff.signalSummary}`}>
              {renderedIcon}
            </span>
          </span>
        </Button>
      </Tooltip>
    );
  }

  return (
    <div className={`diff-signal-summary diff-signal-${diff.signalSummary}`}>
      <Tooltip title={tooltip}>{renderedIcon}</Tooltip>
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
