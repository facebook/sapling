/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import {bookmarksDataStorage} from './BookmarksData';
import {Tooltip} from './Tooltip';
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
  tooltip,
}: {
  children: ReactNode;
  special?: boolean;
  fullLength?: boolean;
  tooltip?: string;
}) {
  const inner = (
    <Tag
      xstyle={[special === true && styles.special, fullLength === true && styles.fullLength]}
      title={tooltip == null && typeof children === 'string' ? children : undefined}>
      {children}
    </Tag>
  );
  return tooltip ? <Tooltip title={tooltip}>{inner}</Tooltip> : inner;
}

export function Bookmarks({
  bookmarks,
  special,
}: {
  bookmarks: ReadonlyArray<string | {value: string; description: string}>;
  special?: boolean;
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
            <Bookmark key={value} special={special} tooltip={tooltip}>
              {value}
            </Bookmark>
          );
        })}
    </>
  );
}
