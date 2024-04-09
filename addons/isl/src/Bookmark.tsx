/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ContextMenuItem} from 'shared/ContextMenu';

import {bookmarksDataStorage} from './BookmarksData';
import {Tooltip} from './Tooltip';
import {tracker} from './analytics';
import {Tag} from './components/Tag';
import {T} from './i18n';
import {BookmarkDeleteOperation} from './operations/BookmarkDeleteOperation';
import {useRunOperation} from './operationsState';
import * as stylex from '@stylexjs/stylex';
import {useAtomValue} from 'jotai';
import {useContextMenu} from 'shared/ContextMenu';

const styles = stylex.create({
  stable: {
    backgroundColor: 'var(--list-hover-background)',
    color: 'var(--list-hover-foreground)',
  },
  fullLength: {
    maxWidth: 'unset',
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
}: {
  children: string;
  kind: BookmarkKind;
  fullLength?: boolean;
  tooltip?: string;
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
      xstyle={[kind === 'stable' && styles.stable, fullLength === true && styles.fullLength]}
      title={tooltip == null ? bookmark : undefined}>
      {bookmark}
    </Tag>
  );
  return tooltip ? <Tooltip title={tooltip}>{inner}</Tooltip> : inner;
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
