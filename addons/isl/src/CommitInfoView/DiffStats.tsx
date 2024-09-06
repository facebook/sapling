/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from '../types';

import {Row} from '../ComponentUtils';
import {T, t} from '../i18n';
import {SLOC_THRESHOLD_FOR_SPLIT_SUGGESTIONS} from '../sloc/diffStatConstants';
import {
  useFetchSignificantLinesOfCode,
  useFetchPendingSignificantLinesOfCode,
} from '../sloc/useFetchSignificantLinesOfCode';
import * as stylex from '@stylexjs/stylex';
import {ErrorBoundary} from 'isl-components/ErrorNotice';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';

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
  const strictSignificantLinesOfCode = slocInfo?.strictSloc;

  if (isLoading && significantLinesOfCode == null) {
    return <LoadingDiffStatsView />;
  } else if (!isLoading && significantLinesOfCode == null) {
    return null;
  }

  return (
    <ResolvedDiffStatsView
      significantLinesOfCode={significantLinesOfCode}
      strictSignificantLinesOfCode={strictSignificantLinesOfCode}
    />
  );
}

export function PendingDiffStats({showWarning = false}: {showWarning?: boolean}) {
  return (
    <ErrorBoundary>
      <PendingDiffStatsView showWarning={showWarning} />
    </ErrorBoundary>
  );
}

export function PendingDiffStatsView({showWarning = false}: {showWarning?: boolean}) {
  const {slocInfo, isLoading} = useFetchPendingSignificantLinesOfCode();
  const significantLinesOfCode = slocInfo?.sloc;
  const strictSignificantLinesOfCode = slocInfo?.strictSloc;

  if (isLoading && significantLinesOfCode == null) {
    return <LoadingDiffStatsView />;
  } else if (!isLoading && significantLinesOfCode == null) {
    return null;
  }

  return (
    <ResolvedDiffStatsView
      significantLinesOfCode={significantLinesOfCode}
      strictSignificantLinesOfCode={strictSignificantLinesOfCode}
      showWarning={showWarning}
    />
  );
}

function ResolvedDiffStatsView({
  significantLinesOfCode,
  strictSignificantLinesOfCode,
  showWarning,
}: {
  significantLinesOfCode: number | undefined;
  strictSignificantLinesOfCode: number | undefined;
  showWarning?: boolean;
}) {
  if (significantLinesOfCode == null) {
    return null;
  }
  const extras =
    showWarning &&
    strictSignificantLinesOfCode &&
    strictSignificantLinesOfCode > SLOC_THRESHOLD_FOR_SPLIT_SUGGESTIONS ? (
      <Tooltip
        title={t(
          //formatting this on multiple lines to look good in the tooltip
          `Consider unselecting some of these changes.

Small Diffs lead to quicker review times.
`,
        )}>
        <Icon icon="warning" color="yellow" />
      </Tooltip>
    ) : null;

  return (
    <DiffStatsView extras={extras}>
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
