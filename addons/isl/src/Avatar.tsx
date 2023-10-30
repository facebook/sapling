/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import serverAPI from './ClientToServerAPI';
import {t} from './i18n';
import platform from './platform';
import {latestCommits} from './serverAPIState';
import {selector, useRecoilValueLoadable} from 'recoil';

const uniqueAuthors = selector<Array<string>>({
  key: 'uniqueAuthors',
  get: ({get}): Array<string> => {
    const commits = get(latestCommits);
    const authors = commits
      .filter(commit => commit.phase !== 'public')
      .map(commit => commit.author);
    const unique = new Set(authors);
    return Array.from(unique);
  },
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

const avatars = selector<Map<string, string>>({
  key: 'avatars',
  get: async ({get}) => {
    const authors = get(uniqueAuthors);

    const found = getCachedAvatars(authors);
    if (found != null) {
      return found;
    }

    serverAPI.postMessage({
      type: 'fetchAvatars',
      authors,
    });
    const result = await serverAPI.nextMessageMatching('fetchedAvatars', () => true);

    storeCachedAvatars(result.avatars);

    return result.avatars;
  },
});

export function Avatar({username}: {username: string}) {
  const storage = useRecoilValueLoadable(avatars);
  const img = storage.valueMaybe()?.get(username);

  return (
    <div className="commit-avatar">
      {img == null ? null : (
        <img src={img} width={14} height={14} alt={t("$user's avatar photo")} />
      )}
    </div>
  );
}
