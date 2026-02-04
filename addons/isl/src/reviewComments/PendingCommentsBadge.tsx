/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as stylex from '@stylexjs/stylex';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtomValue} from 'jotai';
import {spacing, radius, font} from '../../../components/theme/tokens.stylex';
import {pendingCommentsAtom} from './pendingCommentsState';

export type PendingCommentsBadgeProps = {
  prNumber: string;
};

const styles = stylex.create({
  badge: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: spacing.quarter,
    padding: `${spacing.quarter} ${spacing.half}`,
    backgroundColor: 'var(--graphite-accent-subtle, rgba(74, 144, 226, 0.15))',
    color: 'var(--graphite-accent, #4a90e2)',
    borderRadius: radius.round,
    fontSize: font.small,
    fontWeight: 500,
  },
});

/**
 * Badge component showing the count of pending comments for a PR.
 * Displays nothing when there are no pending comments.
 */
export function PendingCommentsBadge({prNumber}: PendingCommentsBadgeProps) {
  const pendingComments = useAtomValue(pendingCommentsAtom(prNumber));
  const count = pendingComments.length;

  if (count === 0) {
    return null;
  }

  const tooltipText = `${count} pending comment${count === 1 ? '' : 's'} - will be submitted with review`;

  return (
    <Tooltip title={tooltipText}>
      <span {...stylex.props(styles.badge)}>
        <Icon icon="comment" />
        {count}
      </span>
    </Tooltip>
  );
}
