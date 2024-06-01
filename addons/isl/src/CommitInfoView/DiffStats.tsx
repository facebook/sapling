/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ChangedFile, CommitInfo} from '../types';

import {Row} from '../ComponentUtils';
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

export function DiffStats({commit}: Props) {
  const significantLinesOfCode = useFetchSignificantLinesOfCode(commit);
  return <DiffStatsView significantLinesOfCode={significantLinesOfCode} />;
}

export function PendingDiffStats({commit, selectedFiles}: Props & {selectedFiles: ChangedFile[]}) {
  const significantLinesOfCode = useFetchPendingSignificantLinesOfCode(commit, selectedFiles);
  return <DiffStatsView significantLinesOfCode={significantLinesOfCode} />;
}

function DiffStatsView({significantLinesOfCode}: {significantLinesOfCode?: number}) {
  if (significantLinesOfCode == null) {
    return null;
  }
  return (
    <Row xstyle={styles.locInfo}>
      <Icon icon="code" />
      <T replace={{$num: significantLinesOfCode}}>$num lines</T>
      <Tooltip
        title={t(
          'This number reflects significant lines of code: non-blank, non-generated additions + deletions',
        )}>
        <Icon icon="info" />
      </Tooltip>
    </Row>
  );
}
