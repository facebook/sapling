/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ContextMenuItem} from 'shared/ContextMenu';
import type {CommitInfo} from './types';

import * as stylex from '@stylexjs/stylex';
import {Button} from 'isl-components/Button';
import {Column} from 'isl-components/Flex';
import {Icon} from 'isl-components/Icon';
import {Tag} from 'isl-components/Tag';
import {TextField} from 'isl-components/TextField';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtomValue} from 'jotai';
import {useState} from 'react';
import {useContextMenu} from 'shared/ContextMenu';
import {spacing} from '../../components/theme/tokens.stylex';
import {tracker} from './analytics';
import {bookmarksDataStorage, recommendedBookmarksAtom} from './BookmarksData';
import {Row} from './ComponentUtils';
import {useFeatureFlagSync} from './featureFlags';
import {T, t} from './i18n';
import {Internal} from './Internal';
import {BookmarkCreateOperation} from './operations/BookmarkCreateOperation';
import {BookmarkDeleteOperation} from './operations/BookmarkDeleteOperation';
import {useRunOperation} from './operationsState';
import {latestSuccessorUnlessExplicitlyObsolete} from './successionUtils';
import {showModal} from './useModal';

const styles = stylex.create({
  stable: {
    backgroundColor: 'var(--list-hover-background)',
    color: 'var(--list-hover-foreground)',
  },
  fullLength: {
    maxWidth: 'unset',
  },
  bookmarkTag: {
    maxWidth: '300px',
    display: 'flex',
    alignItems: 'center',
    gap: spacing.quarter,
  },
  modalButtonBar: {
    justifyContent: 'flex-end',
  },
});

export type BookmarkKind = 'remote' | 'local' | 'stable';

const logged = new Set<string>();
function logExposureOncePerSession(location: string) {
  if (logged.has(location)) {
    return;
  }
  tracker.track('SawStableLocation', {extras: {location}});
  logged.add(location);
}

export function Bookmark({
  children,
  kind,
  fullLength,
  tooltip,
  isRecommended,
}: {
  children: string;
  kind: BookmarkKind;
  fullLength?: boolean;
  tooltip?: string | React.ReactNode;
  isRecommended?: boolean;
}) {
  const bookmark = children;
  const contextMenu = useContextMenu(makeBookmarkContextMenuOptions);
  const runOperation = useRunOperation();

  function makeBookmarkContextMenuOptions() {
    const items: Array<ContextMenuItem> = [];
    if (kind === 'local') {
      items.push({
        label: <T replace={{$book: bookmark}}>Delete Bookmark "$book"</T>,
        onClick: () => {
          runOperation(new BookmarkDeleteOperation(bookmark));
        },
      });
    }
    return items;
  }

  if (kind === 'stable') {
    logExposureOncePerSession(bookmark);
  }

  const inner = (
    <Tag
      onContextMenu={contextMenu}
      xstyle={[
        kind === 'stable' && styles.stable,
        styles.bookmarkTag,
        fullLength === true && styles.fullLength,
      ]}>
      {isRecommended && (
        <Icon icon="star-full" size="XS" style={{display: 'flex', height: '12px'}} />
      )}
      {bookmark}
    </Tag>
  );
  return tooltip ? <Tooltip title={tooltip}>{inner}</Tooltip> : inner;
}

export function AllBookmarksTruncated({
  stable,
  remote,
  local,
}: {
  stable: ReadonlyArray<string | {value: string; description: string}>;
  remote: ReadonlyArray<string>;
  local: ReadonlyArray<string>;
}) {
  const bookmarksData = useAtomValue(bookmarksDataStorage);
  const recommendedBookmarks = useAtomValue(recommendedBookmarksAtom);
  const recommendedBookmarksGK = useFeatureFlagSync(Internal.featureFlags?.RecommendedBookmarks);

  const finalBookmarks = (
    [
      ['local', local],
      ['remote', remote],
      ['stable', stable],
    ] as const
  )
    .map(([kind, bookmarks]) =>
      bookmarks
        .filter(
          bookmark =>
            !bookmarksData.hiddenRemoteBookmarks.includes(
              typeof bookmark === 'string' ? bookmark : bookmark.value,
            ),
        )
        .map(bookmark => {
          const value = typeof bookmark === 'string' ? bookmark : bookmark.value;
          const isRecommended = recommendedBookmarksGK && recommendedBookmarks.has(value);
          const tooltip =
            typeof bookmark === 'string'
              ? isRecommended
                ? Internal.RecommendedBookmarkInfo?.()
                : undefined
              : bookmark.description;

          return {value, kind, tooltip, isRecommended};
        }),
    )
    .flat();
  const NUM_TO_SHOW = 3;
  const shownBookmarks = finalBookmarks.slice(0, NUM_TO_SHOW);
  const hiddenBookmarks = finalBookmarks.slice(NUM_TO_SHOW);
  const numTruncated = hiddenBookmarks.length;

  return (
    <>
      {shownBookmarks.map(({value, kind, tooltip, isRecommended}) => (
        <Bookmark key={value} kind={kind} tooltip={tooltip} isRecommended={isRecommended}>
          {value}
        </Bookmark>
      ))}
      {numTruncated > 0 && (
        <Tooltip
          component={() => (
            <Column alignStart>
              {hiddenBookmarks.map(({value, kind, tooltip, isRecommended}) => (
                <Bookmark
                  key={value}
                  kind={kind}
                  tooltip={tooltip}
                  isRecommended={isRecommended}
                  fullLength>
                  {value}
                </Bookmark>
              ))}
            </Column>
          )}>
          <Tag>
            <T replace={{$n: numTruncated}}>+$n more</T>
          </Tag>
        </Tooltip>
      )}
    </>
  );
}

export function Bookmarks({
  bookmarks,
  kind,
}: {
  bookmarks: ReadonlyArray<string | {value: string; description: string}>;
  kind: BookmarkKind;
}) {
  const bookmarksData = useAtomValue(bookmarksDataStorage);
  return (
    <>
      {bookmarks
        .filter(
          bookmark =>
            !bookmarksData.hiddenRemoteBookmarks.includes(
              typeof bookmark === 'string' ? bookmark : bookmark.value,
            ),
        )
        .map(bookmark => {
          const value = typeof bookmark === 'string' ? bookmark : bookmark.value;
          const tooltip = typeof bookmark === 'string' ? undefined : bookmark.description;
          return (
            <Bookmark key={value} kind={kind} tooltip={tooltip}>
              {value}
            </Bookmark>
          );
        })}
    </>
  );
}

export async function createBookmarkAtCommit(commit: CommitInfo) {
  await showModal({
    type: 'custom',
    title: <T>Create Bookmark</T>,
    component: ({returnResultAndDismiss}: {returnResultAndDismiss: (data?: undefined) => void}) => (
      <CreateBookmarkAtCommitModal commit={commit} dismiss={returnResultAndDismiss} />
    ),
  });
}

function CreateBookmarkAtCommitModal({commit, dismiss}: {commit: CommitInfo; dismiss: () => void}) {
  const runOperation = useRunOperation();
  const [bookmark, setBookmark] = useState('');
  return (
    <>
      <TextField
        autoFocus
        value={bookmark}
        onChange={e => setBookmark(e.currentTarget.value)}
        aria-label={t('Bookmark Name')}
      />
      <Row {...stylex.props(styles.modalButtonBar)}>
        <Button
          onClick={() => {
            dismiss();
          }}>
          <T>Cancel</T>
        </Button>
        <Button
          primary
          onClick={() => {
            runOperation(
              new BookmarkCreateOperation(
                latestSuccessorUnlessExplicitlyObsolete(commit),
                bookmark,
              ),
            );
            dismiss();
          }}
          disabled={bookmark.trim().length === 0}>
          <T>Create</T>
        </Button>
      </Row>
    </>
  );
}
