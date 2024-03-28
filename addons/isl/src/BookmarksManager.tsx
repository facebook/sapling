/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {BookmarkKind} from './Bookmark';
import type {ReactNode} from 'react';

import {Bookmark} from './Bookmark';
import {bookmarksDataStorage, fetchedStablesAtom, remoteBookmarks} from './BookmarksData';
import {Column, ScrollY} from './ComponentUtils';
import {DropdownFields} from './DropdownFields';
import {useCommandEvent} from './ISLShortcuts';
import {Kbd} from './Kbd';
import {Tooltip} from './Tooltip';
import {Checkbox} from './components/Checkbox';
import {T} from './i18n';
import {spacing} from './tokens.stylex';
import * as stylex from '@stylexjs/stylex';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useAtom, useAtomValue} from 'jotai';
import {Icon} from 'shared/Icon';
import {KeyCode, Modifier} from 'shared/KeyboardShortcuts';
import {notEmpty} from 'shared/utils';

const styles = stylex.create({
  bookmarkGroup: {
    alignItems: 'flex-start',
    marginInline: spacing.pad,
  },
  fields: {
    alignItems: 'flex-start',
    marginInline: spacing.pad,
  },
});

export function BookmarksManagerMenu() {
  const additionalToggles = useCommandEvent('ToggleBookmarksManagerDropdown');
  const bookmarks = useAtomValue(remoteBookmarks);
  if (bookmarks.length < 2) {
    // No use showing bookmarks menu if there's only one remote bookmark
    return null;
  }
  return (
    <Tooltip
      component={dismiss => <BookmarksManager dismiss={dismiss} />}
      trigger="click"
      placement="bottom"
      group="topbar"
      title={
        <T replace={{$shortcut: <Kbd keycode={KeyCode.M} modifiers={[Modifier.ALT]} />}}>
          Bookmarks Manager ($shortcut)
        </T>
      }
      additionalToggles={additionalToggles}>
      <VSCodeButton appearance="icon" data-testid="bookmarks-manager-button">
        <Icon icon="bookmark" />
      </VSCodeButton>
    </Tooltip>
  );
}

function BookmarksManager(_props: {dismiss: () => void}) {
  const bookmarks = useAtomValue(remoteBookmarks);
  const stableLocations = useAtomValue(fetchedStablesAtom);
  return (
    <DropdownFields
      title={<T>Bookmarks Manager</T>}
      icon="bookmark"
      data-testid="bookmarks-manager-dropdown">
      <BookmarksList title={<T>Remote Bookmarks</T>} names={bookmarks} kind="remote" />
      <BookmarksList
        title={<T>Stable Locations</T>}
        names={stableLocations?.special?.map(info => info.value?.name).filter(notEmpty) ?? []}
        kind="stable"
      />
    </DropdownFields>
  );
}

function BookmarksList({
  names,
  title,
  kind,
}: {
  names: Array<string>;
  title: ReactNode;
  kind: BookmarkKind;
}) {
  const [bookmarksData, setBookmarksData] = useAtom(bookmarksDataStorage);
  if (names.length == 0) {
    return null;
  }

  return (
    <Column xstyle={styles.bookmarkGroup}>
      <strong>{title}</strong>
      <ScrollY maxSize={300}>
        <Column xstyle={styles.bookmarkGroup}>
          {names.map(bookmark => (
            <Checkbox
              key={bookmark}
              checked={!bookmarksData.hiddenRemoteBookmarks.includes(bookmark)}
              onChange={checked => {
                const shouldBeDeselected = !checked;
                let hiddenRemoteBookmarks = bookmarksData.hiddenRemoteBookmarks;
                if (shouldBeDeselected) {
                  hiddenRemoteBookmarks = [...hiddenRemoteBookmarks, bookmark];
                } else {
                  hiddenRemoteBookmarks = hiddenRemoteBookmarks.filter(b => b !== bookmark);
                }
                setBookmarksData({...bookmarksData, hiddenRemoteBookmarks});
              }}>
              <Bookmark fullLength key={bookmark} kind={kind}>
                {bookmark}
              </Bookmark>
            </Checkbox>
          ))}
        </Column>
      </ScrollY>
    </Column>
  );
}
