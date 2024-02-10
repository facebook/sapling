/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import serverAPI from './ClientToServerAPI';
import {t} from './i18n';
import {atomWithRefresh, refreshAtom} from './jotaiUtils';
import platform from './platform';
import {latestCommits} from './serverAPIState';
import {atom, useAtomValue} from 'jotai';
import {atomFamily} from 'jotai/utils';
import {isPromise} from 'shared/utils';

const uniqueAuthors = atom<Array<string>>(get => {
  const commits = get(latestCommits);
  const authors = commits.filter(commit => commit.phase !== 'public').map(commit => commit.author);
  const unique = new Set(authors);
  return Array.from(unique);
});

type StoredAvatarData = {
  lastFetched: number;
  avatars: Array<[string, string]>;
};

function getCachedAvatars(authors: Array<string>): undefined | Map<string, string> {
  try {
    const found = platform.getTemporaryState('avatars') as StoredAvatarData;
    if (
      // not yet cached
      found == null ||
      // cache expired
      new Date().valueOf() - new Date(found.lastFetched).valueOf() > 24 * 60 * 60 * 1000
    ) {
      return undefined;
    }
    const storedAvatars = new Map(found.avatars);

    // make sure the cache is exhaustive
    if (authors.every(author => storedAvatars.has(author))) {
      return storedAvatars;
    }
  } catch {
    // ignore
  }
  return undefined;
}
function storeCachedAvatars(avatars: Map<string, string>) {
  platform.setTemporaryState('avatars', {
    lastFetched: new Date().valueOf(),
    avatars: Array.from(avatars),
  } as StoredAvatarData);
}

const avatars = atomWithRefresh<Map<string, string> | Promise<Map<string, string>>>(get => {
  const authors = get(uniqueAuthors);

  const found = getCachedAvatars(authors);
  if (found != null) {
    return found;
  }

  // PERF: This might be O(N^2) if we see new authors over time (ex. infinite scroll).
  // Consider avoiding fetching "known" authors.
  serverAPI.postMessage({
    type: 'fetchAvatars',
    authors,
  });

  return (async () => {
    const result = await serverAPI.nextMessageMatching('fetchedAvatars', () => true);

    storeCachedAvatars(result.avatars);
    refreshAtom(avatars);
    return result.avatars;
  })();
});

const avatarUrl = atomFamily((username: string) =>
  atom(get => {
    const storage = get(avatars);
    if (isPromise(storage)) {
      // TODO: Consider loading from cache here.
      return undefined;
    }
    return storage.get(username);
  }),
);

export function Avatar({username}: {username: string}) {
  const img = useAtomValue(avatarUrl(username));

  return (
    <div className="commit-avatar">
      {img == null ? null : (
        <img src={img} width={14} height={14} alt={t("$user's avatar photo")} />
      )}
    </div>
  );
}

/** Render as a SVG pattern */
export function AvatarPattern({
  username,
  size,
  id,
  fallbackFill,
}: {
  username: string;
  size: number;
  id: string;
  fallbackFill: string;
}) {
  const img = useAtomValue(avatarUrl(username));
  return (
    <pattern
      id={id}
      patternUnits="userSpaceOnUse"
      width={size}
      height={size}
      x={-size / 2}
      y={-size / 2}>
      <rect width={size} height={size} fill={fallbackFill} strokeWidth={0} />
      <image href={img} width={size} height={size} />
    </pattern>
  );
}
