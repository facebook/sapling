/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {UsernameQueryData, UsernameQueryVariables} from '../generated/graphql';
import type {Loadable} from 'recoil';

import {UsernameQuery} from '../generated/graphql';
import {ALL_DB_NAMES_EVER} from './databaseInfo';
import {broadcastLogoutMessage, subscribeToLogout} from './logoutBroadcastChannel';
import queryGraphQL from './queryGraphQL';
import {atom, selector, DefaultValue, RecoilLoadable} from 'recoil';
import {createRequestHeaders} from 'shared/github/auth';
import rejectAfterTimeout from 'shared/rejectAfterTimeout';

/*
 * We take the following approach to ensure that when a user hits Logout, we
 * remove all of the user's local data stored in the browser for the hostname on
 * which ReviewStack is being served. Likewise, when a user logs in, we also
 * remove any existing local data for the hostname so that it cannot interfere
 * with the user's experience.
 *
 * Because we store user data in a combination of localStorage and indexedDB,
 * and because indexedDB has async access patterns while localStorage is
 * accessed synchronously, we delete all of the data from indexedDB before we
 * call `localStorage.clear()`. In this way:
 *
 * - If "github.token" is set in localStorage, this signals that there may be
 *   corresponding user data in indexedDB.
 * - If "github.token" is not set in localStorage, then ReviewStack has deleted
 *   all user data from indexedDB (or has never written any).
 *
 * Updates to `gitHubPersonalAccessToken` must go through
 * `gitHubTokenPersistence`. Note that `gitHubTokenPersistence` ensures that
 * `clearAllLocalData()` has completed successfully before
 * `localStorage.setItem("github.token")` is called.
 */

const GITHUB_TOKEN_PROPERTY = 'github.token';
const GITHUB_HOSTNAME_PROPERTY = 'github.hostname';

/**
 * This should not be accessed directly: readers and writers must go
 * through gitHubTokenPersistence.
 */
const gitHubPersonalAccessToken = atom<Loadable<string | null>>({
  key: 'gitHubPersonalAccessToken',
  default: RecoilLoadable.of(localStorage.getItem(GITHUB_TOKEN_PROPERTY)),
  // The Loadable may be backed by a Promise, which has mutable state.
  dangerouslyAllowMutability: true,
  effects: [
    // Handle "logout" events.
    ({setSelf}) => {
      // If we receive a "logout" event, we know that it must have come from
      // another tab. We should present the UI as if the user hit Logout in this
      // tab, even if localStorage has not been cleared yet.
      const unsubscribe = subscribeToLogout(() => {
        const token = localStorage.getItem(GITHUB_TOKEN_PROPERTY);
        if (token == null) {
          // It appears the tab where the user hit Logout already cleared
          // localStorage, so set ourselves to null.
          setSelf(RecoilLoadable.of(null));
        } else {
          // Return a Loadable that is backed by a Promise that is not fulfilled
          // until we observe localStorage being cleared.
          setSelf(
            RecoilLoadable.of(
              new Promise(resolve => {
                window.addEventListener('storage', (event: StorageEvent) => {
                  // We want to be sure we only respond to localStorage events.
                  if (event.storageArea !== localStorage) {
                    return;
                  }

                  // If localStorage.clear() was called, event.key will be null.
                  if (
                    event.key === null ||
                    (event.key === GITHUB_TOKEN_PROPERTY && event.newValue == null)
                  ) {
                    resolve(null);
                  }
                });
              }),
            ),
          );
        }
      });
      return unsubscribe;
    },
    // Write new, non-null value to localStorage.
    ({onSet}) => {
      onSet(loadable => {
        loadable.toPromise().then(token => {
          if (token != null) {
            localStorage.setItem(GITHUB_TOKEN_PROPERTY, token);
          }
        });
      });
    },
  ],
});

/**
 * Before writing this value via `useSetRecoilState()`, ensure that
 * the `gitHubHostname` atom has been written first.
 *
 * TODO(mbolin): Modify this selector so that it takes {token, hostname} as a
 * pair and change the use of the underlying storage to ensure they are
 * persisted atomically.
 */
export const gitHubTokenPersistence = selector<string | null>({
  key: 'gitHubTokenPersistence',
  get: ({get}) => get(gitHubPersonalAccessToken),
  set: ({get, set}, tokenArg) => {
    // If DefaultValue is passed in, this called via a reset action, so treat
    // it as if the value were null.
    const token = tokenArg instanceof DefaultValue ? null : tokenArg;
    if (token == null) {
      broadcastLogoutMessage();
    }

    // Whenever the value of the token changes, either null to non-null or
    // non-null to null, we want to clear out any data that may have been
    // persisted locally.
    //
    // - For a user logging in, we do not want to pick up any state written
    //   previously by a potentially nefarious user.
    // - For a user logging out, we want to remove all of their data.
    const hostname = get(gitHubHostname);
    const promise: Promise<string | null> = clearAllLocalData().then(() => {
      // localStorage was just cleared, so we need to ensure the GitHub hostname
      // is persisted in localStorage.
      if (token != null && hostname != null) {
        localStorage.setItem(GITHUB_HOSTNAME_PROPERTY, hostname);
      }
      return token;
    });
    const loadable = RecoilLoadable.of(promise);
    set(gitHubPersonalAccessToken, loadable);
  },
});

/**
 * If all databases are not dropped within this time window, then it seems
 * unlikely that the operation will succeed, as it is likely an issue where
 * something else is holding a connection open, preventing deletion.
 */
const DELETE_ALL_DATABASES_TIMEOUT_MS = 10_000;

/**
 * Remove all local data stored for the user, which means clearing out
 * everything in indexedDB and localStorage.
 *
 * If this Promise rejects, there are no guarantees that all of the user's data
 * was deleted.
 */
async function clearAllLocalData(): Promise<void> {
  if (typeof indexedDB !== 'undefined') {
    await rejectAfterTimeout(
      dropAllDatabases(indexedDB),
      DELETE_ALL_DATABASES_TIMEOUT_MS,
      `databases not dropped within ${DELETE_ALL_DATABASES_TIMEOUT_MS}ms`,
    );
  }
  localStorage.clear();
}

async function dropAllDatabases(indexedDB: IDBFactory): Promise<unknown> {
  let databaseNames: string[];
  if (indexedDB.databases == null) {
    // As of Nov 16, 2022, Firefox does not support indexedDB.databases():
    // https://bugzilla.mozilla.org/show_bug.cgi?id=934640.
    databaseNames = [...ALL_DB_NAMES_EVER];
  } else {
    const databases = await indexedDB.databases();
    databaseNames = databases.map(db => {
      const {name} = db;
      if (name != null) {
        return name;
      } else {
        throw Error('IDBDatabaseInfo with no name');
      }
    });
  }

  return Promise.all(
    databaseNames.map(name => {
      return new Promise((resolve, reject) => {
        // Note: deleteDatabase() is considered a "success" even if no database
        // exists with the specified name.
        const request = indexedDB.deleteDatabase(name);
        request.onerror = event => reject(`failed to delete db ${name}: ${event}`);
        request.onsuccess = event => resolve(`successfully deleted db ${name}: ${event}`);
      });
    }),
  );
}

export const gitHubUsername = selector<string | null>({
  key: 'gitHubUsername',
  get: ({get}) => {
    const token = get(gitHubTokenPersistence);
    if (token == null) {
      return null;
    }

    const key = deriveLocalStoragePropForUsername(token);
    const username = localStorage.getItem(key);
    if (username != null) {
      return username;
    }

    const graphQLEndpoint = get(gitHubGraphQLEndpoint);
    return queryGraphQL<UsernameQueryData, UsernameQueryVariables>(
      UsernameQuery,
      {},
      createRequestHeaders(token),
      graphQLEndpoint,
    ).then(data => {
      const username = data.viewer.login;
      localStorage.setItem(key, username);
      return username;
    });
  },
});

export const gitHubHostname = atom<string>({
  key: 'gitHubHostname',
  default: localStorage.getItem(GITHUB_HOSTNAME_PROPERTY) || 'github.com',
});

export const isConsumerGitHub = selector<boolean>({
  key: 'isConsumerGitHub',
  get: ({get}) => get(gitHubHostname) === 'github.com',
});

export const gitHubGraphQLEndpoint = selector<string>({
  key: 'gitHubGraphQLEndpoint',
  get: ({get}) => {
    const hostname = get(gitHubHostname);
    return createGraphQLEndpointForHostname(hostname);
  },
});

export function createGraphQLEndpointForHostname(hostname: string): string {
  // According to GitHub's documentation:
  //
  // https://docs.github.com/en/enterprise-server@3.6/graphql/guides/introduction-to-graphql#discovering-the-graphql-api
  //
  // The URL to use for the GraphQL API is:
  //
  //   http(s)://HOSTNAME/api/graphql
  //
  // Though for the GHE instance we tested, both of these appear to work:
  //
  //   https://api.HOSTNAME/graphql
  //   https://HOSTNAME/api/graphql
  //
  // And for consumer GitHub, trying to make API requests using curl:
  //
  //   https://api.github.com/graphql works
  //   https://github.com/api/graphql fails with "Cookies must be enabled to use GitHub."
  //
  // While it is possible that https://api.HOSTNAME/graphql always works for
  // both enterprise and consume GitHub, we'll go with what is documented to
  // play it safe.
  if (hostname === 'github.com') {
    return 'https://api.github.com/graphql';
  } else {
    return `https://${hostname}/api/graphql`;
  }
}

function deriveLocalStoragePropForUsername(token: string): string {
  return `username.${token}`;
}
