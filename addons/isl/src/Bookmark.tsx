/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import {bookmarksDataStorage} from './BookmarksData';
import {Tag} from './components/Tag';
import * as stylex from '@stylexjs/stylex';
import {useAtomValue} from 'jotai';

const styles = stylex.create({
  special: {
    backgroundColor: 'var(--list-hover-background)',
    color: 'var(--list-hover-foreground)',
  },
  fullLength: {
    maxWidth: 'unset',
  },
});

export function Bookmark({
  children,
  special,
  fullLength,
}: {
  children: ReactNode;
  special?: boolean;
  fullLength?: boolean;
}) {
  return (
    <Tag
      xstyle={[special === true && styles.special, fullLength === true && styles.fullLength]}
      title={typeof children === 'string' ? children : undefined}>
      {children}
    </Tag>
  );
}

export function Bookmarks({bookmarks}: {bookmarks: ReadonlyArray<string>}) {
  const bookmarksData = useAtomValue(bookmarksDataStorage);
  return (
    <>
      {bookmarks
        .filter(bookmark => !bookmarksData.hiddenRemoteBookmarks.includes(bookmark))
        .map(bookmark => (
          <Bookmark key={bookmark}>{bookmark}</Bookmark>
        ))}
    </>
  );
}
