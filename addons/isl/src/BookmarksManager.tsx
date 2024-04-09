/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {BookmarkKind} from './Bookmark';
import type {StableInfo} from './types';
import type {ReactNode} from 'react';

import {Bookmark} from './Bookmark';
import {bookmarksDataStorage, fetchedStablesAtom, remoteBookmarks} from './BookmarksData';
import {Column, ScrollY} from './ComponentUtils';
import {DropdownFields} from './DropdownFields';
import {useCommandEvent} from './ISLShortcuts';
import {Kbd} from './Kbd';
import {Subtle} from './Subtle';
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
  container: {
    alignItems: 'flex-start',
    gap: spacing.double,
  },
  bookmarkGroup: {
    alignItems: 'flex-start',
    marginInline: spacing.half,
    gap: spacing.half,
  },
  description: {
    marginBottom: spacing.half,
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

  return (
    <DropdownFields
      title={<T>Bookmarks Manager</T>}
      icon="bookmark"
      data-testid="bookmarks-manager-dropdown">
      <Column xstyle={styles.container}>
        <Section
          title={<T>Remote Bookmarks</T>}
          description={<T>Uncheck remote bookmarks you don't use to hide them</T>}>
          <BookmarksList bookmarks={bookmarks} kind="remote" />
        </Section>
        <StableLocationsSection />
      </Column>
    </DropdownFields>
  );
}

function StableLocationsSection() {
  const stableLocations = useAtomValue(fetchedStablesAtom);

  return (
    <Section
      title={<T>Stable Locations</T>}
      description={
        <T>
          Commits that have had successful builds and warmed up caches for a particular build target
        </T>
      }>
      <BookmarksList
        bookmarks={stableLocations?.special?.map(info => info.value).filter(notEmpty) ?? []}
        kind="stable"
      />
    </Section>
  );
}

function Section({
  title,
  description,
  children,
}: {
  title: ReactNode;
  description?: ReactNode;
  children: ReactNode;
}) {
  return (
    <Column xstyle={styles.bookmarkGroup}>
      <strong>{title}</strong>
      {description && <Subtle {...stylex.props(styles.description)}>{description}</Subtle>}
      {children}
    </Column>
  );
}

function BookmarksList({
  bookmarks,
  kind,
}: {
  bookmarks: Array<string | StableInfo>;
  kind: BookmarkKind;
}) {
  const [bookmarksData, setBookmarksData] = useAtom(bookmarksDataStorage);
  if (bookmarks.length == 0) {
    return null;
  }

  return (
    <Column xstyle={styles.bookmarkGroup}>
      <ScrollY maxSize={300}>
        <Column xstyle={styles.bookmarkGroup}>
          {bookmarks.map(bookmark => {
            const name = typeof bookmark === 'string' ? bookmark : bookmark.name;
            const tooltip = typeof bookmark === 'string' ? undefined : bookmark.info;
            return (
              <Checkbox
                key={name}
                checked={!bookmarksData.hiddenRemoteBookmarks.includes(name)}
                onChange={checked => {
                  const shouldBeDeselected = !checked;
                  let hiddenRemoteBookmarks = bookmarksData.hiddenRemoteBookmarks;
                  if (shouldBeDeselected) {
                    hiddenRemoteBookmarks = [...hiddenRemoteBookmarks, name];
                  } else {
                    hiddenRemoteBookmarks = hiddenRemoteBookmarks.filter(b => b !== name);
                  }
                  setBookmarksData({...bookmarksData, hiddenRemoteBookmarks});
                }}>
                <Bookmark fullLength key={name} kind={kind} tooltip={tooltip}>
                  {name}
                </Bookmark>
              </Checkbox>
            );
          })}
        </Column>
      </ScrollY>
    </Column>
  );
}
