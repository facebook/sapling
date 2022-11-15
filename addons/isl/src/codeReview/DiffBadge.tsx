/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffId, DiffSummary} from '../types';
import type {UICodeReviewProvider} from './UICodeReviewProvider';
import type {ReactNode} from 'react';

import {ExternalLink} from '../ExternalLink';
import {Icon} from '../Icon';
import {Tooltip} from '../Tooltip';
import {t} from '../i18n';
import {diffSummary, codeReviewProvider} from './CodeReviewInfo';
import {openerUrlForDiffUrl} from './github/GitHubUrlOpener';
import {useState, Component, Suspense} from 'react';
import {useRecoilValue} from 'recoil';

import './DiffBadge.css';

/**
 * Component that shows inline summary information about a Diff,
 * such as its status, number of comments, CI state, etc.
 */
export function DiffInfo({diffId}: {diffId: string}) {
  const repo = useRecoilValue(codeReviewProvider);
  if (repo == null) {
    return null;
  }
  return (
    <DiffErrorBoundary provider={repo} diffId={diffId}>
      <Suspense fallback={<DiffSpinner diffId={diffId} provider={repo} />}>
        <DiffInfoInner diffId={diffId} provider={repo} />
      </Suspense>
    </DiffErrorBoundary>
  );
}

export function DiffBadge({
  diff,
  children,
  url,
  provider,
}: {
  diff?: DiffSummary;
  children?: ReactNode;
  url?: string;
  provider: UICodeReviewProvider;
}) {
  const openerUrl = useRecoilValue(openerUrlForDiffUrl(url));
  return (
    <ExternalLink url={openerUrl} className={`diff-badge ${provider.name}-diff-badge`}>
      <provider.DiffBadgeContent diff={diff} children={children} />
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

function DiffInfoInner({diffId, provider}: {diffId: DiffId; provider: UICodeReviewProvider}) {
  const diffInfoResult = useRecoilValue(diffSummary(diffId));
  if (diffInfoResult.error) {
    return <DiffLoadError number={provider.formatDiffNumber(diffId)} provider={provider} />;
  }
  if (diffInfoResult?.value == null) {
    return <DiffSpinner diffId={diffId} provider={provider} />;
  }
  const info = diffInfoResult.value;
  return (
    <div
      className={`diff-info ${provider.name}-diff-info`}
      data-testid={`${provider.name}-diff-info`}>
      <DiffSignalSummary diff={info} />
      <DiffBadge provider={provider} diff={info} url={info.url} />
      <DiffComments diff={info} />
      <DiffNumber>{provider.formatDiffNumber(diffId)}</DiffNumber>
    </div>
  );
}

function DiffNumber({children}: {children: string}) {
  const [showing, setShowing] = useState(false);
  if (!children) {
    return null;
  }

  return (
    <Tooltip trigger="manual" shouldShow={showing} title={t(`Copied ${children} to the clipboard`)}>
      <span
        className="diff-number"
        onClick={() => {
          navigator.clipboard.writeText(children);
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
      icon = 'ellipsis';
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
      icon = 'question';
      tooltip = t(
        'Test Signals were not fully successful for this Diff. See Diff for more details.',
      );
      break;
  }
  return (
    <div className={`diff-signal-summary diff-signal-${diff.signalSummary}`}>
      <Tooltip title={tooltip}>
        <Icon icon={icon} />
      </Tooltip>
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
