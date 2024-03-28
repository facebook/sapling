/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {bookmarksDataStorage} from './BookmarksData';
import {Tooltip} from './Tooltip';
import {tracker} from './analytics';
import {Tag} from './components/Tag';
import * as stylex from '@stylexjs/stylex';
import {useAtomValue} from 'jotai';

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
  if (kind === 'stable') {
    logExposureOncePerSession(children);
  }
  const inner = (
    <Tag
      xstyle={[kind === 'stable' && styles.stable, fullLength === true && styles.fullLength]}
      title={tooltip == null ? children : undefined}>
      {children}
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
