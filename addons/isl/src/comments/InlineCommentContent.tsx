/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffComment, SuggestedChange} from 'isl/src/types';

import * as stylex from '@stylexjs/stylex';
import {Badge} from 'isl-components/Badge';
import {Column, Row} from 'isl-components/Flex';
import {Icon} from 'isl-components/Icon';
import {AvatarImg} from 'isl/src/Avatar';
import {ArchivedReasonType, ArchivedStateType, CodePatchSuggestionStatus} from 'isl/src/types';

const styles = stylex.create({
  avatarColumn: {
    flexShrink: 0,
  },
  avatar: {
    marginBlock: '5px',
  },
  commentColumn: {
    gap: 1,
  },
  commentAuthorRow: {
    gap: 6,
  },
  commentAuthor: {
    lineHeight: '17px',
    textWrap: 'nowrap',
    opacity: 0.75,
  },
  versionAbbr: {
    opacity: 0.5,
    whiteSpace: 'nowrap',
  },
  primaryBadge: {
    backgroundColor: 'var(--button-primary-background)',
    color: 'var(--button-primary-foreground)',
  },
  suggestionBadge: {
    gap: '2px',
  },
});

function getSuggestionBadgeLabel(
  suggestedChange: SuggestedChange | undefined,
): {text: string; isPrimary: boolean} | undefined {
  if (suggestedChange?.patch == null) {
    return undefined;
  }
  if (suggestedChange.status === CodePatchSuggestionStatus.Declined) {
    return {text: 'Rejected', isPrimary: false};
  }
  if (suggestedChange.archivedState === ArchivedStateType.ARCHIVED) {
    switch (suggestedChange.archivedReason) {
      case ArchivedReasonType.APPLIED_IN_EDITOR:
        return {text: 'Applied Inline', isPrimary: true};
      case ArchivedReasonType.APPLIED_MERGED:
      case ArchivedReasonType.APPLIED_STACKED_DIFF:
        return {text: 'Merged', isPrimary: false};
      case ArchivedReasonType.AUTHOR_DISCARDED:
        return {text: 'Rejected', isPrimary: false};
      case ArchivedReasonType.STALE_DIFF_CLOSED:
      case ArchivedReasonType.STALE_FILE_CHANGED:
        return {text: 'Closed', isPrimary: false};
    }
  }
  if (suggestedChange.status === CodePatchSuggestionStatus.Accepted) {
    return {text: 'Accepted', isPrimary: true};
  }
  return {text: 'Suggestion', isPrimary: true};
}

export function CommentCardBadge({
  label,
  icon,
}: {
  label: {text: string; isPrimary: boolean};
  icon: string;
}) {
  return (
    <Badge xstyle={label.isPrimary && styles.primaryBadge}>
      <Row xstyle={styles.suggestionBadge}>
        <span>
          <Icon icon={icon} size="XS" />
        </span>
        {label.text}
      </Row>
    </Badge>
  );
}

export default function InlineCommentContent({
  comment,
  isHeadComment = false,
  isHidden = false,
  isLatestVersion = true,
  versionAbbr,
}: {
  comment: DiffComment;
  isHeadComment?: boolean;
  isHidden?: boolean;
  isLatestVersion?: boolean;
  versionAbbr?: string;
}) {
  const label = getSuggestionBadgeLabel(comment.suggestedChange);
  return (
    <Column alignStart xstyle={styles.commentColumn}>
      <Row xstyle={styles.commentAuthorRow}>
        <Column alignStart xstyle={styles.avatarColumn}>
          <AvatarImg
            username={comment.author}
            url={comment.authorAvatarUri}
            xstyle={styles.avatar}
          />
        </Column>
        <div {...stylex.props(styles.commentAuthor)}>{comment.authorName}</div>
        {label && <CommentCardBadge label={label} icon="code" />}
        {isHidden && (
          <CommentCardBadge label={{text: 'Hidden', isPrimary: false}} icon="eye-closed" />
        )}
        {isHeadComment && !isLatestVersion && versionAbbr && (
          <div {...stylex.props(styles.versionAbbr)}>[Original comment on {versionAbbr}]</div>
        )}
      </Row>
      <Row>{comment.content}</Row>
    </Column>
  );
}
