/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ContextMenuItem} from 'shared/ContextMenu';
import type {InternalTypes} from './InternalTypes';
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
import {
  bookmarksDataStorage,
  recommendedBookmarksAtom,
  recommendedBookmarksAvailableAtom,
  REMOTE_MASTER_BOOKMARK,
} from './BookmarksData';
import {Row} from './ComponentUtils';
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

export function getBookmarkAddons(
  name: string,
  showRecommendedIcon: boolean,
  showWarningOnMaster: boolean,
  tooltipOverride?: string,
): {icon: string | undefined; tooltip: React.ReactNode | undefined} {
  if (showWarningOnMaster && name === REMOTE_MASTER_BOOKMARK) {
    return {icon: 'warning', tooltip: tooltipOverride ?? Internal.MasterBookmarkInfo?.()};
  }
  if (showRecommendedIcon) {
    return {icon: 'star-full', tooltip: tooltipOverride ?? Internal.RecommendedBookmarkInfo?.()};
  }
  return {icon: undefined, tooltip: tooltipOverride};
}

export function Bookmark({
  children,
  kind,
  fullLength,
  tooltip,
  icon,
}: {
  children: string;
  kind: BookmarkKind;
  fullLength?: boolean;
  tooltip?: string | React.ReactNode;
  icon?: string;
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
      {icon && <Icon icon={icon} size="XS" style={{display: 'flex', height: '12px'}} />}
      {bookmark}
    </Tag>
  );
  return tooltip ? <Tooltip title={tooltip}>{inner}</Tooltip> : inner;
}

export function AllBookmarksTruncated({
  stable,
  remote,
  local,
  fullRepoBranch,
}: {
  stable: ReadonlyArray<string | {value: string; description: string; isRecommended?: boolean}>;
  remote: ReadonlyArray<string>;
  local: ReadonlyArray<string>;
  fullRepoBranch?: InternalTypes['FullRepoBranch'] | undefined;
}) {
  const bookmarksData = useAtomValue(bookmarksDataStorage);
  const recommendedBookmarks = useAtomValue(recommendedBookmarksAtom);
  const recommendedBookmarksAvailable = useAtomValue(recommendedBookmarksAvailableAtom);

  const FullRepoBranchBookmark = Internal.FullRepoBranchBookmark;
  const compareFullRepoBranch = Internal.compareFullRepoBranch;

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
        .filter(bookmark =>
          compareFullRepoBranch ? compareFullRepoBranch(fullRepoBranch, bookmark) : true,
        )
        .map(bookmark => {
          const value = typeof bookmark === 'string' ? bookmark : bookmark.value;
          const isRecommended =
            recommendedBookmarks.has(value) ||
            (typeof bookmark === 'object' && bookmark.isRecommended === true);
          const tooltipOverride = typeof bookmark === 'string' ? undefined : bookmark.description;
          const {icon, tooltip} = getBookmarkAddons(
            value,
            isRecommended,
            recommendedBookmarksAvailable,
            tooltipOverride,
          );

          return {value, kind, tooltip, icon};
        }),
    )
    .flat();
  const NUM_TO_SHOW = fullRepoBranch == null ? 3 : 2;
  const shownBookmarks = finalBookmarks.slice(0, NUM_TO_SHOW);
  const hiddenBookmarks = finalBookmarks.slice(NUM_TO_SHOW);
  const numTruncated = hiddenBookmarks.length;

  return (
    <>
      {fullRepoBranch && FullRepoBranchBookmark && (
        <FullRepoBranchBookmark branch={fullRepoBranch} />
      )}
      {shownBookmarks.map(({value, kind, tooltip, icon}) => (
        <Bookmark key={value} kind={kind} tooltip={tooltip} icon={icon}>
          {value}
        </Bookmark>
      ))}
      {numTruncated > 0 && (
        <Tooltip
          component={() => (
            <Column alignStart>
              {hiddenBookmarks.map(({value, kind, tooltip, icon}) => (
                <Bookmark key={value} kind={kind} tooltip={tooltip} icon={icon} fullLength>
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
