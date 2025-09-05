/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TypeaheadResult} from 'isl-components/Types';
import type {ReactNode} from 'react';
import type {BookmarkKind} from './Bookmark';
import type {Result, StableInfo} from './types';

import * as stylex from '@stylexjs/stylex';
import {Banner, BannerKind} from 'isl-components/Banner';
import {Button} from 'isl-components/Button';
import {Checkbox} from 'isl-components/Checkbox';
import {InlineErrorBadge} from 'isl-components/ErrorNotice';
import {Icon} from 'isl-components/Icon';
import {Kbd} from 'isl-components/Kbd';
import {KeyCode, Modifier} from 'isl-components/KeyboardShortcuts';
import {Subtle} from 'isl-components/Subtle';
import {extractTokens} from 'isl-components/Tokens';
import {Tooltip} from 'isl-components/Tooltip';
import {Typeahead} from 'isl-components/Typeahead';
import {atom, useAtom, useAtomValue} from 'jotai';
import React, {useState} from 'react';
import {firstLine, notEmpty} from 'shared/utils';
import {spacing} from '../../components/theme/tokens.stylex';
import {Bookmark} from './Bookmark';
import {
  addManualStable,
  bookmarksDataStorage,
  fetchedStablesAtom,
  remoteBookmarks,
  removeManualStable,
} from './BookmarksData';
import serverAPI from './ClientToServerAPI';
import {Column, Row, ScrollY} from './ComponentUtils';
import {DropdownFields} from './DropdownFields';
import {useCommandEvent} from './ISLShortcuts';
import {Internal} from './Internal';
import {T, t} from './i18n';
import {readAtom} from './jotaiUtils';
import {latestDag} from './serverAPIState';

export const styles = stylex.create({
  container: {
    alignItems: 'flex-start',
    gap: spacing.double,
    width: 500,
    maxWidth: 500,
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
      additionalToggles={additionalToggles.asEventTarget()}>
      <Button icon data-testid="bookmarks-manager-button">
        <Icon icon="bookmark" />
      </Button>
    </Tooltip>
  );
}

function BookmarksManager(_props: {dismiss: () => void}) {
  const bookmarks = useAtomValue(remoteBookmarks);
  const bookmarksData = useAtomValue(bookmarksDataStorage);

  return (
    <DropdownFields
      title={<T>Bookmarks Manager</T>}
      icon="bookmark"
      data-testid="bookmarks-manager-dropdown">
      <Column xstyle={styles.container}>
        {Internal.RecommendedBookmarkSection?.()}
        <Section
          title={<T>Remote Bookmarks</T>}
          description={<T>Uncheck remote bookmarks you don't use to hide them</T>}>
          <BookmarksList
            bookmarks={bookmarks}
            kind="remote"
            disabled={bookmarksData.useRecommendedBookmark}
          />
        </Section>
        <StableLocationsSection />
      </Column>
    </DropdownFields>
  );
}

const latestPublicCommitAtom = atom(get => {
  const dag = get(latestDag);
  const latestHash = dag.heads(dag.public_()).toArray()[0];
  return latestHash ? dag.get(latestHash) : undefined;
});

function stableIsNewerThanMainWarning(latestPublicDate?: Date, info?: Result<StableInfo>) {
  const isNewerThanLatest = info?.value && latestPublicDate && info.value.date > latestPublicDate;
  return isNewerThanLatest ? (
    <Banner kind={BannerKind.warning}>
      <T>Stable is newer than latest pulled commit. Pull to fetch latest.</T>
    </Banner>
  ) : undefined;
}

function StableLocationsSection() {
  const stableLocations = useAtomValue(fetchedStablesAtom);
  const latestPublic = useAtomValue(latestPublicCommitAtom);

  return (
    <Section
      title={<T>Stable Locations</T>}
      description={
        <T>
          Commits that have had successful builds and warmed up caches for a particular build target
        </T>
      }>
      <BookmarksList
        bookmarks={
          stableLocations?.special
            ?.map(info => {
              if (info.value == null) {
                return undefined;
              }
              return {
                ...info.value,
                extra: stableIsNewerThanMainWarning(latestPublic?.date, info),
              };
            })
            .filter(notEmpty) ?? []
        }
        kind="stable"
      />
      {stableLocations?.manual && (
        <BookmarksList
          bookmarks={Object.entries(stableLocations.manual)?.map(([name, info]) => {
            const deleteButton = (
              <Tooltip title={t('Remove this stable location')}>
                <Button
                  icon
                  onClick={e => {
                    removeManualStable(name);
                    e.stopPropagation();
                  }}>
                  <Icon icon="trash" />
                </Button>
              </Tooltip>
            );
            if (info == null) {
              return {
                kind: 'custom',
                custom: (
                  <Row>
                    {name}: <Icon icon="loading" />
                  </Row>
                ),
              };
            }
            if (info.error) {
              return {
                kind: 'custom',
                custom: (
                  <Row>
                    {name}:{' '}
                    <InlineErrorBadge error={info.error}>
                      {firstLine(info.error.toString())}
                    </InlineErrorBadge>
                    {deleteButton}
                  </Row>
                ),
              };
            }
            return {
              ...info.value,
              extra: (
                <Row>
                  {deleteButton}
                  {stableIsNewerThanMainWarning(latestPublic?.date, info)}
                </Row>
              ),
            };
          })}
          kind="stable"
        />
      )}
      {stableLocations?.repoSupportsCustomStables === true && <AddStableLocation />}
    </Section>
  );
}

let typeaheadOptionsPromise: Promise<Result<Array<TypeaheadResult>>> | undefined;
const getStableLocationsTypeaheadOptions = () => {
  if (typeaheadOptionsPromise != null) {
    return typeaheadOptionsPromise;
  }
  typeaheadOptionsPromise = (async () => {
    serverAPI.postMessage({type: 'fetchStableLocationAutocompleteOptions'});
    const result = await serverAPI.nextMessageMatching(
      'fetchedStableLocationAutocompleteOptions',
      () => true,
    );
    return result.result;
  })();
  return typeaheadOptionsPromise;
};

const stableLocationsTypeaheadOptions = atom(getStableLocationsTypeaheadOptions);

function AddStableLocation() {
  const [showingInput, setShowingInput] = useState(false);
  const [query, setQuery] = useState('');
  const addRef = React.useRef<HTMLButtonElement>(null);
  return (
    <div style={{paddingTop: 'var(--pad)'}}>
      {showingInput ? (
        <div>
          <Subtle>{Internal.StableLocationAddInformation?.()}</Subtle>
          <Row>
            <Typeahead
              tokenString={query}
              setTokenString={setQuery}
              fetchTokens={async (query: string) => {
                const fetchStartTimestamp = Date.now();
                const options = await readAtom(stableLocationsTypeaheadOptions);
                const normalized = query.toLowerCase();
                return {
                  fetchStartTimestamp,
                  values:
                    options.value?.filter(
                      opt =>
                        opt.value.toLowerCase().includes(normalized) ||
                        opt.label.toLowerCase().includes(normalized),
                    ) ?? [],
                };
              }}
              onSaveNewToken={() => {
                addRef?.current?.focus();
              }}
              autoFocus
              maxTokens={1}
            />
            <Button
              ref={addRef}
              primary
              onClick={e => {
                // only expect one token
                const [[token]] = extractTokens(query);
                const stable = token.trim();
                if (stable) {
                  addManualStable(stable);
                  setQuery('');
                  setShowingInput(false);
                }
                e.stopPropagation();
              }}>
              <T>Add</T>
            </Button>
          </Row>
        </div>
      ) : (
        <Button
          icon
          onClick={e => {
            e.stopPropagation();
            setShowingInput(true);

            // Start fetching options as soon as we show the typeahead
            getStableLocationsTypeaheadOptions();
          }}>
          <Icon icon="plus" />
          <T>Add Stable Location</T>
        </Button>
      )}
    </div>
  );
}

export function Section({
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
  disabled = false,
}: {
  bookmarks: Array<
    | string
    | (StableInfo & {extra?: ReactNode; kind?: undefined})
    | {kind: 'custom'; custom: ReactNode}
  >;
  kind: BookmarkKind;
  disabled?: boolean;
}) {
  const [bookmarksData, setBookmarksData] = useAtom(bookmarksDataStorage);
  if (bookmarks.length == 0) {
    return null;
  }

  return (
    <ScrollY maxSize={300}>
      <Column xstyle={styles.bookmarkGroup}>
        {bookmarks.map(bookmark => {
          if (typeof bookmark !== 'string' && bookmark.kind === 'custom') {
            return bookmark.custom;
          }
          const name = typeof bookmark === 'string' ? bookmark : bookmark.name;
          const tooltip = typeof bookmark === 'string' ? undefined : bookmark.info;
          const extra = typeof bookmark === 'string' ? undefined : bookmark.extra;
          return (
            <Checkbox
              key={name}
              checked={!bookmarksData.hiddenRemoteBookmarks.includes(name)}
              disabled={disabled}
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
              {extra}
            </Checkbox>
          );
        })}
      </Column>
    </ScrollY>
  );
}
