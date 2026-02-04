/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ParsedDiff} from 'shared/patch/types';
import type {DiffComment, DiffCommentReaction, DiffId} from '../types';

import * as stylex from '@stylexjs/stylex';
import {ErrorNotice} from 'isl-components/ErrorNotice';
import {Icon} from 'isl-components/Icon';
import {Subtle} from 'isl-components/Subtle';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtom, useAtomValue} from 'jotai';
import {useEffect, useState} from 'react';
import {ComparisonType} from 'shared/Comparison';
import {group} from 'shared/utils';
import {colors, font, radius, spacing} from '../../../components/theme/tokens.stylex';
import {AvatarImg} from '../Avatar';
import {SplitDiffTable} from '../ComparisonView/SplitDiffView/SplitDiffHunk';
import {Column, Row} from '../ComponentUtils';
import {Link} from '../Link';
import {T, t} from '../i18n';
import platform from '../platform';
import {RelativeDate} from '../relativeDate';
import {ReplyInput, ThreadResolutionButton} from '../reviewComments';
import {layout} from '../stylexUtils';
import {themeState} from '../theme';
import {diffCommentData} from './codeReviewAtoms';

const styles = stylex.create({
  list: {
    minWidth: '400px',
    maxWidth: '600px',
    maxHeight: '300px',
    overflowY: 'auto',
    alignItems: 'flex-start',
  },
  comment: {
    alignItems: 'flex-start',
    width: 'calc(100% - 12px)',
    backgroundColor: colors.bg,
    padding: '2px 6px',
    borderRadius: radius.round,
  },
  commentInfo: {
    gap: spacing.half,
    marginBlock: spacing.half,
    alignItems: 'flex-start',
  },
  inlineCommentFilename: {
    marginBottom: spacing.half,
  },
  commentContent: {
    whiteSpace: 'pre-wrap',
  },
  left: {
    alignItems: 'end',
    flexShrink: 0,
  },
  author: {
    fontSize: font.small,
    flexShrink: 0,
  },
  avatar: {
    marginBlock: spacing.half,
  },
  byline: {
    display: 'flex',
    flexDirection: 'row',
    gap: spacing.pad,
    alignItems: 'center',
  },
  diffView: {
    marginBlock: spacing.pad,
  },
  replyButton: {
    cursor: 'pointer',
    opacity: 0.7,
    ':hover': {
      opacity: 1,
    },
  },
  resolved: {
    opacity: 0.7,
    borderLeftWidth: '2px',
    borderLeftStyle: 'solid',
    borderLeftColor: colors.grey,
    paddingLeft: spacing.pad,
  },
  collapsedSummary: {
    cursor: 'pointer',
    padding: spacing.pad,
    backgroundColor: colors.subtleHoverDarken,
    borderRadius: radius.round,
    display: 'flex',
    alignItems: 'center',
    gap: spacing.half,
    width: '100%',
  },
  threadHeader: {
    display: 'flex',
    flexDirection: 'row',
    alignItems: 'center',
    gap: spacing.pad,
    marginBottom: spacing.half,
  },
});

function Comment({
  comment,
  isTopLevel,
  onRefresh,
}: {
  comment: DiffComment;
  isTopLevel?: boolean;
  onRefresh?: () => void;
}) {
  const [showReply, setShowReply] = useState(false);
  // Track local resolution state for optimistic UI updates
  const [localIsResolved, setLocalIsResolved] = useState(comment.isResolved);
  // Collapsed state - resolved threads start collapsed
  const [collapsed, setCollapsed] = useState(comment.isResolved === true);

  // If this is a resolved thread and collapsed, show summary
  if (isTopLevel && localIsResolved === true && collapsed) {
    return (
      <div
        {...stylex.props(styles.collapsedSummary)}
        onClick={() => setCollapsed(false)}
        role="button"
        tabIndex={0}
        onKeyDown={e => e.key === 'Enter' && setCollapsed(false)}>
        <Icon icon="check" />
        <Subtle>
          <T>Resolved thread</T>
          {' - '}
          {comment.author}
          {': '}
          {comment.html.replace(/<[^>]*>/g, '').slice(0, 50)}
          {comment.html.replace(/<[^>]*>/g, '').length > 50 ? '...' : ''}
        </Subtle>
        <Icon icon="chevron-down" />
      </div>
    );
  }

  return (
    <Row xstyle={[styles.comment, isTopLevel && localIsResolved === true && styles.resolved]}>
      <Column {...stylex.props(styles.left)}>
        <AvatarImg username={comment.author} url={comment.authorAvatarUri} xstyle={styles.avatar} />
      </Column>
      <Column xstyle={styles.commentInfo}>
        {/* Thread header with author and resolution button */}
        <div {...stylex.props(styles.threadHeader)}>
          <b {...stylex.props(styles.author)}>{comment.author}</b>
          {isTopLevel && comment.threadId && (
            <ThreadResolutionButton
              threadId={comment.threadId}
              isResolved={localIsResolved ?? false}
              onStatusChange={newStatus => {
                setLocalIsResolved(newStatus);
                if (newStatus) {
                  // When resolved, collapse after a brief moment
                  setTimeout(() => setCollapsed(true), 500);
                }
                onRefresh?.();
              }}
            />
          )}
        </div>
        <div>
          {isTopLevel && comment.filename && (
            <Link
              xstyle={styles.inlineCommentFilename}
              onClick={() =>
                comment.filename && platform.openFile(comment.filename, {line: comment.line})
              }>
              {comment.filename}
              {comment.line == null ? '' : ':' + comment.line}
            </Link>
          )}
          <div {...stylex.props(styles.commentContent)}>
            <div className="rendered-markup" dangerouslySetInnerHTML={{__html: comment.html}} />
          </div>
          {comment.suggestedChange != null && comment.suggestedChange.patch != null && (
            <InlineDiff patch={comment.suggestedChange.patch} />
          )}
        </div>
        <Subtle {...stylex.props(styles.byline)}>
          <RelativeDate date={comment.created} />
          <Reactions reactions={comment.reactions} />
          {localIsResolved === true ? (
            <span
              {...stylex.props(styles.replyButton)}
              onClick={() => setCollapsed(!collapsed)}
              role="button"
              tabIndex={0}
              onKeyDown={e => e.key === 'Enter' && setCollapsed(!collapsed)}>
              <T>Resolved</T>
            </span>
          ) : localIsResolved === false ? (
            <span>
              <T>Unresolved</T>
            </span>
          ) : null}
          {comment.threadId && (
            <Tooltip title={t('Reply to thread')}>
              <span
                {...stylex.props(styles.replyButton)}
                onClick={() => setShowReply(true)}
                role="button"
                tabIndex={0}
                onKeyDown={e => e.key === 'Enter' && setShowReply(true)}>
                <Icon icon="comment" />
              </span>
            </Tooltip>
          )}
        </Subtle>
        {showReply && comment.threadId && (
          <ReplyInput
            threadId={comment.threadId}
            onCancel={() => setShowReply(false)}
            onSuccess={() => {
              setShowReply(false);
              onRefresh?.();
            }}
          />
        )}
        {comment.replies.map((reply, i) => (
          <Comment key={i} comment={reply} onRefresh={onRefresh} />
        ))}
      </Column>
    </Row>
  );
}

const useThemeHook = () => useAtomValue(themeState);

function InlineDiff({patch}: {patch: ParsedDiff}) {
  const path = patch.newFileName ?? '';
  return (
    <div {...stylex.props(styles.diffView)}>
      <div className="split-diff-view">
        <SplitDiffTable
          patch={patch}
          path={path}
          ctx={{
            collapsed: false,
            id: {
              comparison: {type: ComparisonType.HeadChanges},
              path,
            },
            setCollapsed: () => null,
            display: 'unified',
            useThemeHook,
          }}
        />
      </div>
    </div>
  );
}

const emoji: Record<DiffCommentReaction['reaction'], string> = {
  LIKE: '👍',
  WOW: '😮',
  SORRY: '🤗',
  LOVE: '❤️',
  HAHA: '😆',
  ANGER: '😡',
  SAD: '😢',
  // GitHub reactions
  CONFUSED: '😕',
  EYES: '👀',
  HEART: '❤️',
  HOORAY: '🎉',
  LAUGH: '😄',
  ROCKET: '🚀',
  THUMBS_DOWN: '👎',
  THUMBS_UP: '👍',
};

function Reactions({reactions}: {reactions: Array<DiffCommentReaction>}) {
  if (reactions.length === 0) {
    return null;
  }
  const groups = Object.entries(group(reactions, r => r.reaction)).filter(
    (group): group is [DiffCommentReaction['reaction'], DiffCommentReaction[]] =>
      (group[1]?.length ?? 0) > 0,
  );
  groups.sort((a, b) => b[1].length - a[1].length);
  const total = groups.reduce((last, g) => last + g[1].length, 0);
  // Show only the 3 most used reactions as emoji, even if more are used
  const icons = groups.slice(0, 2).map(g => <span>{emoji[g[0]]}</span>);
  const names = reactions.map(r => r.name);
  return (
    <Tooltip title={names.join(', ')}>
      <Row style={{gap: spacing.half}}>
        <span style={{letterSpacing: '-2px'}}>{icons}</span>
        <span>{total}</span>
      </Row>
    </Tooltip>
  );
}

export default function DiffCommentsDetails({diffId}: {diffId: DiffId}) {
  const [comments, refresh] = useAtom(diffCommentData(diffId));
  useEffect(() => {
    // make sure we fetch whenever loading the UI again
    refresh();
  }, [refresh]);

  if (comments.state === 'loading') {
    return (
      <div>
        <Icon icon="loading" />
      </div>
    );
  }
  if (comments.state === 'hasError') {
    return (
      <div>
        <ErrorNotice title={t('Failed to fetch comments')} error={comments.error as Error} />
      </div>
    );
  }
  return (
    <div {...stylex.props(layout.flexCol, styles.list)}>
      {comments.data.map((comment, i) => (
        <Comment key={i} comment={comment} isTopLevel onRefresh={refresh} />
      ))}
    </div>
  );
}
