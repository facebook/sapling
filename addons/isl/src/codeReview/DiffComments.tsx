/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffId, DiffComment, DiffCommentReaction} from '../types';
import type {ParsedDiff} from 'shared/patch/parse';

import {colors, font, radius, spacing} from '../../../components/theme/tokens.stylex';
import {AvatarImg} from '../Avatar';
import serverAPI from '../ClientToServerAPI';
import {SplitDiffTable} from '../ComparisonView/SplitDiffView/SplitDiffHunk';
import {Column, Row} from '../ComponentUtils';
import {Link} from '../Link';
import {t} from '../i18n';
import {atomFamilyWeak, atomLoadableWithRefresh} from '../jotaiUtils';
import foundPlatform from '../platform';
import {RelativeDate} from '../relativeDate';
import {layout} from '../stylexUtils';
import * as stylex from '@stylexjs/stylex';
import {ErrorNotice} from 'isl-components/ErrorNotice';
import {Icon} from 'isl-components/Icon';
import {Subtle} from 'isl-components/Subtle';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtom} from 'jotai';
import {useEffect} from 'react';
import {ComparisonType} from 'shared/Comparison';
import {group} from 'shared/utils';

const diffCommentData = atomFamilyWeak((diffId: DiffId) =>
  atomLoadableWithRefresh(async () => {
    serverAPI.postMessage({
      type: 'fetchDiffComments',
      diffId,
    });

    const result = await serverAPI.nextMessageMatching(
      'fetchedDiffComments',
      msg => msg.diffId === diffId,
    );
    if (result.comments.error != null) {
      throw new Error(result.comments.error.toString());
    }
    return result.comments.value;
  }),
);

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
    gap: spacing.half,
  },
  diffView: {
    marginBlock: spacing.pad,
  },
});

function Comment({comment, isTopLevel}: {comment: DiffComment; isTopLevel?: boolean}) {
  return (
    <Row xstyle={styles.comment}>
      <Column {...stylex.props(styles.left)}>
        <AvatarImg username={comment.author} url={comment.authorAvatarUri} xstyle={styles.avatar} />
      </Column>
      <Column xstyle={styles.commentInfo}>
        <b {...stylex.props(styles.author)}>{comment.author}</b>
        <div>
          {isTopLevel && comment.filename && (
            <Link
              xstyle={styles.inlineCommentFilename}
              onClick={() =>
                comment.filename && foundPlatform.openFile(comment.filename, {line: comment.line})
              }>
              {comment.filename}
              {comment.line == null ? '' : ':' + comment.line}
            </Link>
          )}
          <div {...stylex.props(styles.commentContent)}>
            <div className="rendered-markup" dangerouslySetInnerHTML={{__html: comment.html}} />
          </div>
          {comment.suggestedChange != null && <InlineDiff patch={comment.suggestedChange} />}
        </div>
        <Subtle {...stylex.props(styles.byline)}>
          <RelativeDate date={comment.created} />
          <Reactions reactions={comment.reactions} />
        </Subtle>
        {comment.replies.map((reply, i) => (
          <Comment key={i} comment={reply} />
        ))}
      </Column>
    </Row>
  );
}

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
            // we don't have the rest of the contents of the suggestion
            supportsExpandingContext: false,
            display: 'unified',
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
        <Comment key={i} comment={comment} isTopLevel />
      ))}
    </div>
  );
}
