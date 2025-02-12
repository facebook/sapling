/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from '../types';

import * as stylex from '@stylexjs/stylex';
import {ErrorBoundary} from 'isl-components/ErrorNotice';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {Row} from '../ComponentUtils';
import {T, t} from '../i18n';
import {
  useFetchPendingSignificantLinesOfCode,
  useFetchSignificantLinesOfCode,
} from '../sloc/useFetchSignificantLinesOfCode';

type Props = {commit: CommitInfo};
const styles = stylex.create({
  locInfo: {
    alignItems: 'center',
    fontWeight: 'bold',
    textTransform: 'lowercase',
    fontSize: '85%',
    opacity: 0.9,
    gap: 'var(--halfpad)',
  },
});
export function LoadingDiffStatsView() {
  return (
    <DiffStatsView>
      <Icon icon="loading" size="XS" />
      <T>lines</T>
    </DiffStatsView>
  );
}
export function DiffStats({commit}: Props) {
  const {slocInfo, isLoading} = useFetchSignificantLinesOfCode(commit);
  const significantLinesOfCode = slocInfo?.sloc;

  if (isLoading && significantLinesOfCode == null) {
    return <LoadingDiffStatsView />;
  } else if (!isLoading && significantLinesOfCode == null) {
    return null;
  }
  return <ResolvedDiffStatsView significantLinesOfCode={significantLinesOfCode} />;
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
  const significantLinesOfCode = slocInfo?.sloc;

  if (isLoading && significantLinesOfCode == null) {
    return <LoadingDiffStatsView />;
  } else if (!isLoading && significantLinesOfCode == null) {
    return null;
  }
  return <ResolvedDiffStatsView significantLinesOfCode={significantLinesOfCode} />;
}

function ResolvedDiffStatsView({
  significantLinesOfCode,
}: {
  significantLinesOfCode: number | undefined;
}) {
  if (significantLinesOfCode == null) {
    return null;
  }

  return (
    <DiffStatsView>
      <T replace={{$num: significantLinesOfCode}}>$num lines</T>
    </DiffStatsView>
  );
}

function DiffStatsView({extras, children}: {extras?: React.ReactNode; children: React.ReactNode}) {
  return (
    <Row xstyle={styles.locInfo}>
      <Icon icon="code" />
      {children}
      <Tooltip
        title={t(
          'This number reflects significant lines of code: non-blank, non-generated additions + deletions',
        )}>
        <Icon icon="info" />
      </Tooltip>
      {extras}
    </Row>
  );
}
