/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffId, DiffComment} from '../types';

import {AvatarImg} from '../Avatar';
import serverAPI from '../ClientToServerAPI';
import {Column, FlexRow} from '../ComponentUtils';
import {ErrorNotice} from '../ErrorNotice';
import {Link} from '../Link';
import {Subtle} from '../Subtle';
import {t} from '../i18n';
import {atomFamilyWeak, atomLoadableWithRefresh} from '../jotaiUtils';
import foundPlatform from '../platform';
import {RelativeDate} from '../relativeDate';
import {layout} from '../stylexUtils';
import {colors, font, radius, spacing} from '../tokens.stylex';
import * as stylex from '@stylexjs/stylex';
import {useAtom} from 'jotai';
import {useEffect} from 'react';
import {Icon} from 'shared/Icon';

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
  },
  inlineCommentFilename: {
    marginBottom: spacing.half,
  },
  commentContent: {
    whiteSpace: 'pre-wrap',
  },
  left: {
    alignItems: 'end',
  },
  author: {
    fontSize: font.small,
  },
  avatar: {
    borderRadius: radius.full,
    border: '2px solid',
    borderColor: colors.fg,
    marginBlock: spacing.half,
  },
});

function Comment({comment, isTopLevel}: {comment: DiffComment; isTopLevel?: boolean}) {
  return (
    <FlexRow {...stylex.props(styles.comment)}>
      <Column {...stylex.props(styles.left)}>
        <AvatarImg
          username={comment.author}
          url={comment.authorAvatarUri}
          {...stylex.props(styles.avatar)}
        />
      </Column>
      <Column {...stylex.props(styles.commentInfo)}>
        <b {...stylex.props(styles.author)}>{comment.author}</b>
        <div>
          {isTopLevel && comment.filename && (
            <Link
              {...stylex.props(styles.inlineCommentFilename)}
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
        </div>
        <Subtle>
          <RelativeDate date={comment.created} />
        </Subtle>
        {comment.replies.map((reply, i) => (
          <Comment key={i} comment={reply} />
        ))}
      </Column>
    </FlexRow>
  );
}

export function DiffCommentsDetails({diffId}: {diffId: DiffId}) {
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
