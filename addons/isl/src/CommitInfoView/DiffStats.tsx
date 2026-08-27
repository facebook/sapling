/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo, SlocDelta} from '../types';

import {ErrorBoundary} from 'isl-components/ErrorNotice';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {cn} from 'shared/cn';
import {Row} from '../ComponentUtils';
import {t} from '../i18n';
import {
  useFetchPendingSignificantLinesOfCode,
  useFetchSignificantLinesOfCode,
} from '../sloc/useFetchSignificantLinesOfCode';
import css from './DiffStats.module.css';

type Props = {commit: CommitInfo};
export function LoadingDiffStatsView() {
  return (
    <DiffStatsView>
      <Icon icon="loading" size="XS" />
      <span className={cn(css.insertions, css.placeholder)}>+–</span>
      <span className={cn(css.deletions, css.placeholder)}>−–</span>
    </DiffStatsView>
  );
}
export function DiffStats({commit}: Props) {
  const {slocInfo, isLoading} = useFetchSignificantLinesOfCode(commit);
  const sloc = slocInfo?.sloc;

  if (isLoading && sloc == null) {
    return <LoadingDiffStatsView />;
  } else if (!isLoading && sloc == null) {
    return null;
  }
  return <ResolvedDiffStatsView sloc={sloc} />;
}

export function PendingDiffStats() {
  return (
    <ErrorBoundary>
      <PendingDiffStatsView />
    </ErrorBoundary>
  );
}

export function PendingDiffStatsView() {
  const {slocInfo, isLoading} = useFetchPendingSignificantLinesOfCode();
  const sloc = slocInfo?.sloc;

  if (isLoading && sloc == null) {
    return <LoadingDiffStatsView />;
  } else if (!isLoading && sloc == null) {
    return null;
  }
  return <ResolvedDiffStatsView sloc={sloc} />;
}

function ResolvedDiffStatsView({sloc}: {sloc: SlocDelta | undefined}) {
  if (sloc == null) {
    return null;
  }

  return (
    <DiffStatsView>
      <span className={css.insertions}>+{sloc.insertions}</span>
      <span className={css.deletions}>−{sloc.deletions}</span>
    </DiffStatsView>
  );
}

function DiffStatsView({extras, children}: {extras?: React.ReactNode; children: React.ReactNode}) {
  return (
    <Row className={css.locInfo}>
      <Icon icon="code" />
      {children}
      <Tooltip
        title={t(
          'These numbers reflect significant lines of code: non-blank, non-generated additions and deletions',
        )}>
        <Icon icon="info" />
      </Tooltip>
      {extras}
    </Row>
  );
}
