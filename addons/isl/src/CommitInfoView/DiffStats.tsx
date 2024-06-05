/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from '../types';

import {Row} from '../ComponentUtils';
import {SuspenseBoundary} from '../SuspenseBoundary';
import {Tooltip} from '../Tooltip';
import {T, t} from '../i18n';
import {
  useFetchPendingSignificantLinesOfCode,
  useFetchSignificantLinesOfCode,
} from '../sloc/useFetchSignificantLinesOfCode';
import * as stylex from '@stylexjs/stylex';
import {Icon} from 'shared/Icon';

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
  const significantLinesOfCode = useFetchSignificantLinesOfCode(commit);
  return <ResolvedDiffStatsView significantLinesOfCode={significantLinesOfCode} />;
}

export function PendingDiffStats({commit}: Props) {
  return (
    <SuspenseBoundary fallback={<LoadingDiffStatsView />}>
      <PendingDiffStatsView commit={commit} />
    </SuspenseBoundary>
  );
}

export function PendingDiffStatsView({commit}: Props) {
  const significantLinesOfCode = useFetchPendingSignificantLinesOfCode(commit);
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

function DiffStatsView({children}: {children: React.ReactNode}) {
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
    </Row>
  );
}
