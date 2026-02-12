/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * This file contains Jotai atoms for the ReviewStack application.
 */

import type {
  CheckRunFragment,
  LabelFragment,
  StackPullRequestFragment,
  UserFragment,
  UserHomePageQueryData,
  UserHomePageQueryVariables,
  UsernameQueryData,
  UsernameQueryVariables,
} from '../generated/graphql';
import type GitHubClient from '../github/GitHubClient';
import type {DiffCommitIDs, DiffWithCommitIDs, CommitChange} from '../github/diffTypes';
import type {
  GitHubPullRequestReviewThread,
  PullRequest,
  PullRequestReviewComment,
  CommitData,
  PullRequestCommitItem,
} from '../github/pullRequestTimelineTypes';
import type {PullsQueryInput, PullsWithPageInfo} from '../github/pullsTypes';
import type {CommitComparison} from '../github/restApiTypes';
import type {
  Blob,
  Commit,
  DateTime,
  ForcePushEvent,
  GitObjectID,
  ID,
  Version,
  VersionCommit,
} from '../github/types';
import type {SaplingPullRequestBody} from '../saplingStack';

import {lineToPositionAtom} from '../diffServiceClient';
import {DiffSide, UsernameQuery, UserHomePageQuery} from '../generated/graphql';
import {pullRequestNumbersFromBody} from '../ghstackUtils';
import CachingGitHubClient, {openDatabase} from '../github/CachingGitHubClient';
import GraphQLGitHubClient from '../github/GraphQLGitHubClient';
import {ALL_DB_NAMES_EVER} from '../github/databaseInfo';
import {diffCommitWithParent, diffCommits} from '../github/diff';
import {diffVersions} from '../github/diffVersions';
import {createGraphQLEndpointForHostname} from '../github/gitHubCredentials';
import {broadcastLogoutMessage, subscribeToLogout} from '../github/logoutBroadcastChannel';
import queryGraphQL from '../github/queryGraphQL';
import {parseSaplingStackBody} from '../saplingStack';
import {getPathForChange, getTreeEntriesForChange} from '../utils';
import {atom} from 'jotai';
import {atomWithStorage} from 'jotai/utils';
import {atomFamily} from 'jotai-family';
import {createRequestHeaders} from 'shared/github/auth';
import rejectAfterTimeout from 'shared/rejectAfterTimeout';
import {notEmpty} from 'shared/utils';

// =============================================================================
// Theme Atoms
// =============================================================================

/**
 *
 * See https://primer.style/react/theming#color-modes-and-color-schemes
 * Note that "day" is the default. Currently, we choose not to include "auto"
 * because <ThemeProvider> does not appear to support an event to tell us
 * when the colorMode changes.
 */
export type SupportedPrimerColorMode = 'day' | 'night';

const LOCAL_STORAGE_KEY = 'reviewstack-color-mode';

export const primerColorModeAtom = atomWithStorage<SupportedPrimerColorMode>(
  LOCAL_STORAGE_KEY,
  'day',
);

// =============================================================================
// GitHub Credentials (migrated from gitHubCredentials.ts)
// =============================================================================

const GITHUB_TOKEN_PROPERTY = 'github.token';
const GITHUB_HOSTNAME_PROPERTY = 'github.hostname';

/**
 * If all databases are not dropped within this time window, then it seems
 * unlikely that the operation will succeed.
 */
const DELETE_ALL_DATABASES_TIMEOUT_MS = 10_000;

/**
 * Drop all IndexedDB databases.
 */
async function dropAllDatabases(indexedDB: IDBFactory): Promise<unknown> {
  let databaseNames: string[];
  if (indexedDB.databases == null) {
    // Firefox doesn't support indexedDB.databases()
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
        const request = indexedDB.deleteDatabase(name);
        request.onerror = event => reject(`failed to delete db ${name}: ${event}`);
        request.onsuccess = event => resolve(`successfully deleted db ${name}: ${event}`);
      });
    }),
  );
}

/**
 * Clear all local data (indexedDB and localStorage).
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

/**
 * Represents the state of the GitHub token - can be loading, has value, or has error.
 */
export type GitHubTokenState =
  | {state: 'loading'; promise: Promise<string | null>}
  | {state: 'hasValue'; value: string | null}
  | {state: 'hasError'; error: unknown};

/**
 * Primitive atom holding the token state.
 * This manages the loading/settled states for token operations.
 */
export const gitHubTokenStateAtom = atom<GitHubTokenState>({
  state: 'hasValue',
  value: localStorage.getItem(GITHUB_TOKEN_PROPERTY),
});

/**
 * Listener registration atom - when subscribed, sets up cross-tab logout listener.
 * This uses onMount to set up the BroadcastChannel listener.
 */
export const gitHubTokenListenerAtom = atom(
  get => get(gitHubTokenStateAtom),
  (get, set) => {
    // This is the onMount initializer - set up cross-tab logout listener
    const unsubscribe = subscribeToLogout(() => {
      const token = localStorage.getItem(GITHUB_TOKEN_PROPERTY);
      if (token == null) {
        // localStorage already cleared by another tab
        set(gitHubTokenStateAtom, {state: 'hasValue', value: null});
      } else {
        // Wait for localStorage to be cleared
        const promise = new Promise<string | null>(resolve => {
          const handler = (event: StorageEvent) => {
            if (event.storageArea !== localStorage) {
              return;
            }
            if (event.key === null || (event.key === GITHUB_TOKEN_PROPERTY && event.newValue == null)) {
              window.removeEventListener('storage', handler);
              resolve(null);
            }
          };
          window.addEventListener('storage', handler);
        });
        set(gitHubTokenStateAtom, {state: 'loading', promise});
        promise.then(value => {
          set(gitHubTokenStateAtom, {state: 'hasValue', value});
        });
      }
    });
    return unsubscribe;
  },
);
gitHubTokenListenerAtom.onMount = setSelf => setSelf();

/**
 *
 * Writable atom for getting/setting the GitHub token with proper data clearing.
 * - On get: Returns the current token value (or a promise if loading)
 * - On set: Clears all local data first, then sets the token
 */
export const gitHubTokenPersistenceAtom = atom(
  get => {
    const state = get(gitHubTokenStateAtom);
    switch (state.state) {
      case 'hasValue':
        return state.value;
      case 'loading':
        // Return the promise so consumers using loadable can see loading state
        return state.promise;
      case 'hasError':
        throw state.error;
    }
  },
  (get, set, token: string | null) => {
    if (token == null) {
      broadcastLogoutMessage();
    }

    // Get the hostname before clearing localStorage
    const hostname = get(gitHubHostnameAtom);

    // Create a promise for the async clearing operation
    const promise: Promise<string | null> = clearAllLocalData().then(() => {
      // Restore hostname and set new token if provided
      if (token != null && hostname != null) {
        localStorage.setItem(GITHUB_HOSTNAME_PROPERTY, hostname);
        localStorage.setItem(GITHUB_TOKEN_PROPERTY, token);
      }
      return token;
    });

    // Set loading state
    set(gitHubTokenStateAtom, {state: 'loading', promise});

    // When promise resolves, set the final value
    promise.then(
      value => set(gitHubTokenStateAtom, {state: 'hasValue', value}),
      error => set(gitHubTokenStateAtom, {state: 'hasError', error}),
    );
  },
);

/**
 * The hostname for the GitHub instance. Defaults to 'github.com' for consumer
 * GitHub, but can be set to an enterprise hostname.
 */
export const gitHubHostnameAtom = atomWithStorage<string>(
  GITHUB_HOSTNAME_PROPERTY,
  'github.com',
);

/**
 *
 * Derived atom that indicates if the current hostname is consumer GitHub.
 * Used to determine behavior differences between consumer and enterprise GitHub.
 */
export const isConsumerGitHubAtom = atom<boolean>(get => {
  return get(gitHubHostnameAtom) === 'github.com';
});

/**
 *
 * Derives the GraphQL endpoint URL from the hostname.
 */
export const gitHubGraphQLEndpointAtom = atom<string>(get => {
  const hostname = get(gitHubHostnameAtom);
  return createGraphQLEndpointForHostname(hostname);
});

/**
 * Helper to derive localStorage key for cached username.
 */
function deriveLocalStoragePropForUsername(token: string): string {
  return `username.${token}`;
}

/**
 *
 * Fetches the GitHub username for the current token.
 * Caches the username in localStorage to avoid repeated API calls.
 */
export const gitHubUsernameAtom = atom<Promise<string | null>>(async get => {
  const tokenState = get(gitHubTokenStateAtom);
  let token: string | null = null;

  if (tokenState.state === 'hasValue') {
    token = tokenState.value;
  } else if (tokenState.state === 'loading') {
    token = await tokenState.promise;
  } else {
    throw tokenState.error;
  }

  if (token == null) {
    return null;
  }

  const key = deriveLocalStoragePropForUsername(token);
  const cachedUsername = localStorage.getItem(key);
  if (cachedUsername != null) {
    return cachedUsername;
  }

  const graphQLEndpoint = get(gitHubGraphQLEndpointAtom);
  const data = await queryGraphQL<UsernameQueryData, UsernameQueryVariables>(
    UsernameQuery,
    {},
    createRequestHeaders(token),
    graphQLEndpoint,
  );
  const username = data.viewer.login;
  localStorage.setItem(key, username);
  return username;
});

// =============================================================================
// GitHub Organization and Repository
// =============================================================================

export type GitHubOrgAndRepo = {
  org: string;
  repo: string;
};

export const gitHubOrgAndRepoAtom = atom<GitHubOrgAndRepo | null>(null);

// =============================================================================
// GitHub Client
// =============================================================================

const databaseConnectionAtom = atom<Promise<IDBDatabase>>(() => {
  return openDatabase();
});

export const gitHubClientAtom = atom<Promise<GitHubClient | null>>(async get => {
  // Note: gitHubTokenPersistence and gitHubHostname are Recoil atoms from gitHubCredentials.
  // We access their default values directly since they're based on localStorage.
  // This is a temporary bridge during the migration.
  // IMPORTANT: The keys must match those used in gitHubCredentials.ts
  const token = localStorage.getItem('github.token');
  const orgAndRepo = get(gitHubOrgAndRepoAtom);

  if (token != null && orgAndRepo != null) {
    const {org, repo} = orgAndRepo;
    const db = await get(databaseConnectionAtom);
    const hostname = localStorage.getItem('github.hostname') ?? 'github.com';
    const client = new GraphQLGitHubClient(hostname, org, repo, token);
    return new CachingGitHubClient(db, client, org, repo);
  } else {
    return null;
  }
});

// =============================================================================
// Pull Request Loading
// =============================================================================

/**
 * Type for pull request loading params.
 */
export type GitHubPullRequestParams = {
  orgAndRepo: GitHubOrgAndRepo;
  number: number;
};

/**
 * Refresh trigger for PR loading. Increment this to force a refetch.
 * This replaces Recoil's `refresh()` API.
 */
export const gitHubPullRequestRefreshTriggerAtom = atomFamily(
  (_params: GitHubPullRequestParams) => atom<number>(0),
  (a, b) => a.orgAndRepo.org === b.orgAndRepo.org &&
            a.orgAndRepo.repo === b.orgAndRepo.repo &&
            a.number === b.number,
);

/**
 *
 * Fetches pull request data for the given params. The PR will be refetched
 * when gitHubPullRequestRefreshTriggerAtom is incremented.
 *
 * Returns null if the PR is not found.
 */
export const gitHubPullRequestForParamsAtom = atomFamily(
  (params: GitHubPullRequestParams) =>
    atom<Promise<PullRequest | null>>(async get => {
      // Read the refresh trigger - when it changes, this atom re-evaluates
      get(gitHubPullRequestRefreshTriggerAtom(params));

      const token = localStorage.getItem('github.token');
      if (token == null) {
        // Return a never-settling promise to indicate we're waiting for auth
         
        return new Promise<PullRequest | null>(() => {});
      }

      const db = await get(databaseConnectionAtom);
      const hostname = localStorage.getItem('github.hostname') ?? 'github.com';
      const {org, repo} = params.orgAndRepo;
      const client = new GraphQLGitHubClient(hostname, org, repo, token);
      const cachingClient = new CachingGitHubClient(db, client, org, repo);

      return cachingClient.getPullRequest(params.number);
    }),
  (a, b) => a.orgAndRepo.org === b.orgAndRepo.org &&
            a.orgAndRepo.repo === b.orgAndRepo.repo &&
            a.number === b.number,
);

// =============================================================================
// Repo Labels
// =============================================================================

/**
 *
 * Search query for filtering repository labels.
 */
export const gitHubRepoLabelsQuery = atom<string>('');

/**
 *
 * Fetches repository labels based on the search query.
 */
export const gitHubRepoLabels = atom<Promise<LabelFragment[]>>(async get => {
  const client = await get(gitHubClientAtom);
  const query = get(gitHubRepoLabelsQuery);
  if (client == null) {
    return [];
  }
  return client.getRepoLabels(query);
});

// =============================================================================
// Repo Assignable Users
// =============================================================================

/**
 *
 * Search query for filtering assignable users.
 */
export const gitHubRepoAssignableUsersQuery = atom<string>('');

/**
 *
 * Fetches assignable users based on the search query.
 */
export const gitHubRepoAssignableUsers = atom<Promise<UserFragment[]>>(async get => {
  const client = await get(gitHubClientAtom);
  const query = get(gitHubRepoAssignableUsersQuery);
  // Get username from localStorage - derive key from token as done in gitHubCredentials.ts
  const token = localStorage.getItem('github.token');
  const username = token != null ? localStorage.getItem(`username.${token}`) : null;
  if (client == null) {
    return [];
  }
  const users = await client.getRepoAssignableUsers(query);
  return users.filter(user => user.login !== username);
});

// =============================================================================
// Comment Thread Navigation
// =============================================================================

/**
 *
 * An atom family that tracks whether a specific comment should be scrolled to.
 * When set to true, the comment will scroll into view and the atom will be
 * reset to false.
 */
export const gitHubPullRequestJumpToCommentIDAtom = atomFamily(
  (_id: ID) => atom<boolean>(false),
  (a, b) => a === b,
);

// =============================================================================
// Pull Request Labels
// =============================================================================

/**
 *
 * Stores the labels associated with the current pull request.
 * Initialized from the pull request data and updated optimistically
 * when labels are added or removed.
 */
export const gitHubPullRequestLabelsAtom = atom<LabelFragment[]>([]);

// =============================================================================
// Pull Request Reviewers
// =============================================================================

/**
 * Type for the pull request reviewers state.
 */
export type PullRequestReviewersList = {
  reviewers: ReadonlyArray<UserFragment>;
  reviewerIDs: ReadonlySet<string>;
};

/**
 *
 * Stores the reviewers associated with the current pull request.
 * Initialized from the pull request data and updated optimistically
 * when reviewers are added or removed.
 */
export const gitHubPullRequestReviewersAtom = atom<PullRequestReviewersList>({
  reviewers: [],
  reviewerIDs: new Set<string>(),
});

// =============================================================================
// GitHub Commit and Pull Request IDs
// =============================================================================

/**
 *
 * The current commit ID being viewed.
 */
export const gitHubCommitIDAtom = atom<GitObjectID | null>(null);

/**
 *
 * The current pull request number being viewed.
 */
export const gitHubPullRequestIDAtom = atom<number | null>(null);

// =============================================================================
// Pull Request
// =============================================================================

/**
 *
 * The current pull request data. Set when navigating to a PR.
 */
export const gitHubPullRequestAtom = atom<PullRequest | null>(null);

/**
 *
 * Derived atom that indicates if the current viewer authored the PR.
 * Used to determine author-specific behavior (e.g., different review actions).
 * For edit permissions, use gitHubPullRequestViewerCanUpdateAtom instead.
 */
export const gitHubPullRequestViewerDidAuthorAtom = atom<boolean>(get => {
  const pullRequest = get(gitHubPullRequestAtom);
  return pullRequest?.viewerDidAuthor ?? false;
});

/**
 * Derived atom that indicates if the current viewer can update the PR.
 * This is true for both authors AND collaborators with appropriate permissions.
 * Use this for edit controls (labels, reviewers, etc.)
 */
export const gitHubPullRequestViewerCanUpdateAtom = atom<boolean>(get => {
  const pullRequest = get(gitHubPullRequestAtom);
  return pullRequest?.viewerCanUpdate ?? false;
});

// =============================================================================
// Current Commit
// =============================================================================

/**
 *
 * Fetches the current commit data based on the commit ID.
 * Used on the /commit/:oid route.
 */
export const gitHubCurrentCommitAtom = atom<Promise<Commit | null>>(async get => {
  const client = await get(gitHubClientAtom);
  const oid = get(gitHubCommitIDAtom);
  return client != null && oid != null ? client.getCommit(oid) : null;
});

/**
 *
 * Computes the diff for the current commit by comparing it with its parent.
 * Used to display the commit diff view.
 */
export const gitHubDiffForCurrentCommitAtom = atom<Promise<DiffWithCommitIDs | null>>(async get => {
  const client = await get(gitHubClientAtom);
  const commit = await get(gitHubCurrentCommitAtom);
  if (client != null && commit != null) {
    return diffCommitWithParent(commit, client);
  } else {
    return null;
  }
});

// =============================================================================
// Diff Commit IDs
// =============================================================================

/**
 * Partially migrated from: gitHubDiffCommitIDs in recoil.ts
 *
 * Extracts the commit IDs from the current diff.
 * This Jotai version handles the commit view case (when there's no pull request).
 * For the PR case, use the Recoil gitHubDiffCommitIDs selector until
 * gitHubPullRequestVersionDiff is migrated.
 */
export const gitHubDiffCommitIDsForCommitViewAtom = atom<Promise<DiffCommitIDs | null>>(
  async get => {
    const pullRequest = get(gitHubPullRequestAtom);
    // Only handle the commit view case (when there's no pull request)
    if (pullRequest != null) {
      // Return null for PR case - consumers should use Recoil selector
      return null;
    }
    const diffWithCommitIDs = await get(gitHubDiffForCurrentCommitAtom);
    return diffWithCommitIDs?.commitIDs ?? null;
  },
);

// =============================================================================
// Pull Request Version Diff Support
// =============================================================================

/**
 *
 * When there is no "before" explicitly selected, the view shows the Diff for
 * the selected "after" version compared to its parent.
 */
export type ComparableVersions = {
  beforeCommitID: GitObjectID | null;
  afterCommitID: GitObjectID;
};

/**
 *
 * Stores the currently selected versions for comparison in a PR.
 * This is a writable atom - when null, a default is computed from versions.
 */
const gitHubPullRequestComparableVersionsBaseAtom = atom<ComparableVersions | null>(null);

/**
 * Derived atom that provides a default value when the base atom is null.
 * The default uses the latest version's head commit as the afterCommitID.
 */
export const gitHubPullRequestComparableVersionsAtom = atom(
  get => {
    const stored = get(gitHubPullRequestComparableVersionsBaseAtom);
    if (stored != null) {
      return stored;
    }

    // Compute default from the pull request's timeline commits
    const pullRequest = get(gitHubPullRequestAtom);
    if (pullRequest == null) {
      return null;
    }

    // Get the latest commit from the PR timeline (same logic as gitHubPullRequestCommitsAtom)
    const commits = (pullRequest.timelineItems?.nodes ?? [])
      .map(item => {
        if (item?.__typename === 'PullRequestCommit') {
          const commit = item as PullRequestCommitItem;
          return commit.commit;
        } else {
          return null;
        }
      })
      .filter(notEmpty);

    const latestCommit = commits[commits.length - 1];
    if (latestCommit == null) {
      return null;
    }

    return {
      beforeCommitID: null,
      afterCommitID: latestCommit.oid,
    };
  },
  (get, set, newValue: ComparableVersions | null) => {
    set(gitHubPullRequestComparableVersionsBaseAtom, newValue);
  },
);

/**
 *
 * Fetches a commit by its OID using the GitHub client.
 */
export const gitHubCommitAtom = atomFamily(
  (oid: GitObjectID) =>
    atom<Promise<Commit | null>>(async get => {
      const client = await get(gitHubClientAtom);
      return client != null ? client.getCommit(oid) : null;
    }),
  (a, b) => a === b,
);

/**
 *
 * For a given commit in a PR, get its merge base commit with the main branch.
 * Used to identify the appropriate base for comparison when generating diffs
 * across versions.
 *
 * Note: This is a simplified version that fetches the base parent directly
 * through the client. The full version in Recoil goes through
 * gitHubPullRequestVersionBaseAndCommits which uses gitHubCommitComparison.
 */
export const gitHubPullRequestCommitBaseParentAtom = atomFamily(
  (commitID: GitObjectID) =>
    atom<Promise<{oid: GitObjectID; committedDate: DateTime} | null>>(async get => {
      const client = await get(gitHubClientAtom);
      const pullRequest = get(gitHubPullRequestAtom);
      if (client == null || pullRequest == null) {
        return null;
      }

      const baseRef = pullRequest.baseRefOid;
      if (baseRef == null) {
        return null;
      }

      // Use commit comparison to find the merge base
      const comparison = await client.getCommitComparison(baseRef, commitID);
      if (comparison == null) {
        return null;
      }

      return {
        oid: comparison.mergeBaseCommit.sha,
        committedDate: comparison.mergeBaseCommit.commit.committer.date,
      };
    }),
  (a, b) => a === b,
);

/**
 *
 * Computes the diff between two commits (base and head).
 */
export const gitHubDiffForCommitsAtom = atomFamily(
  ({baseCommitID, commitID}: {baseCommitID: GitObjectID; commitID: GitObjectID}) =>
    atom<Promise<DiffWithCommitIDs | null>>(async get => {
      const client = await get(gitHubClientAtom);
      if (client == null) {
        return null;
      }

      const [baseCommit, commit] = await Promise.all([
        get(gitHubCommitAtom(baseCommitID)),
        get(gitHubCommitAtom(commitID)),
      ]);

      if (baseCommit == null || commit == null) {
        return null;
      }

      return diffCommits(baseCommit, commit, client);
    }),
  (a, b) => a.baseCommitID === b.baseCommitID && a.commitID === b.commitID,
);

/**
 *
 * Returns the appropriate Diff for the current pull request. By default, it
 * shows the Diff for the head commit of the PR compared to its parent, though
 * if the user has selected a pair of versions via the radio buttons, it returns
 * the Diff between those versions.
 *
 * Note: When comparableVersions is null (initial state before user selection),
 * the Recoil gitHubPullRequestComparableVersions selector computes a default
 * from gitHubPullRequestVersions. Since that logic remains in Recoil during
 * migration, a null here means we should return null and let the Recoil-based
 * default kick in when the component re-renders.
 */
export const gitHubPullRequestVersionDiffAtom = atom<Promise<DiffWithCommitIDs | null>>(
  async get => {
    const client = await get(gitHubClientAtom);
    const comparableVersions = get(gitHubPullRequestComparableVersionsAtom);

    if (client == null || comparableVersions == null) {
      return null;
    }

    const {beforeCommitID, afterCommitID} = comparableVersions;

    // If afterCommitID is empty, we can't compute a diff
    if (afterCommitID === '') {
      return null;
    }

    // Get the base parent for the "after" commit
    const afterBaseParent = await get(gitHubPullRequestCommitBaseParentAtom(afterCommitID));
    const afterBaseCommitID = afterBaseParent?.oid;

    if (beforeCommitID != null) {
      // Comparing two explicit versions
      const beforeBaseParent = await get(gitHubPullRequestCommitBaseParentAtom(beforeCommitID));
      const beforeBaseCommitID = beforeBaseParent?.oid;

      if (beforeBaseCommitID != null && afterBaseCommitID != null) {
        // If the base parents are the same, then there was no rebase and the
        // two versions can be diffed directly
        if (beforeBaseCommitID === afterBaseCommitID) {
          return get(
            gitHubDiffForCommitsAtom({baseCommitID: beforeCommitID, commitID: afterCommitID}),
          );
        }

        // Different base parents - need to diff the versions against their respective bases
        // and then diff the diffs
        const [beforeDiff, afterDiff] = await Promise.all([
          get(
            gitHubDiffForCommitsAtom({baseCommitID: beforeBaseCommitID, commitID: beforeCommitID}),
          ),
          get(gitHubDiffForCommitsAtom({baseCommitID: afterBaseCommitID, commitID: afterCommitID})),
        ]);

        if (beforeDiff != null && afterDiff != null) {
          return {
            diff: diffVersions(beforeDiff.diff, afterDiff.diff),
            commitIDs: {
              before: beforeCommitID,
              after: afterCommitID,
            },
          };
        }
      }
    } else if (afterBaseCommitID != null) {
      // No explicit "before" - compare "after" against its base
      return get(
        gitHubDiffForCommitsAtom({baseCommitID: afterBaseCommitID, commitID: afterCommitID}),
      );
    }

    return null;
  },
);

/**
 *
 * Extracts the commit IDs from the current diff.
 * Handles both commit view (single commit) and PR view (version comparison).
 */
export const gitHubDiffCommitIDsAtom = atom<Promise<DiffCommitIDs | null>>(async get => {
  const pullRequest = get(gitHubPullRequestAtom);
  if (pullRequest != null) {
    // PR case - use version diff
    const diffWithCommitIDs = await get(gitHubPullRequestVersionDiffAtom);
    return diffWithCommitIDs?.commitIDs ?? null;
  } else {
    // Commit view case
    const diffWithCommitIDs = await get(gitHubDiffForCurrentCommitAtom);
    return diffWithCommitIDs?.commitIDs ?? null;
  }
});

// =============================================================================
// Pull Request Versions
// =============================================================================

/**
 * Internal atom: extracts the base ref OID from the pull request.
 */
const gitHubPullRequestBaseRefAtom = atom<GitObjectID | null>(get => {
  const pullRequest = get(gitHubPullRequestAtom);
  return pullRequest?.baseRefOid ?? null;
});

/**
 * Internal atom: extracts commit data from the pull request timeline.
 */
const gitHubPullRequestCommitsAtom = atom<CommitData[]>(get => {
  const pullRequest = get(gitHubPullRequestAtom);
  return (pullRequest?.timelineItems?.nodes ?? [])
    .map(item => {
      if (item?.__typename === 'PullRequestCommit') {
        const commit = item as PullRequestCommitItem;
        return commit.commit;
      } else {
        return null;
      }
    })
    .filter(notEmpty);
});

/**
 * Internal atom: extracts force push events from the pull request timeline.
 */
const gitHubPullRequestForcePushesAtom = atom<ForcePushEvent[]>(get => {
  const pullRequest = get(gitHubPullRequestAtom);
  return (pullRequest?.timelineItems?.nodes ?? [])
    .map(item => {
      if (item?.__typename === 'HeadRefForcePushedEvent') {
        const {createdAt, beforeCommit, afterCommit} = item;
        if (createdAt == null || beforeCommit == null || afterCommit == null) {
          return null;
        }
        return {
          createdAt,
          beforeCommit: beforeCommit.oid,
          beforeCommittedDate: beforeCommit.committedDate,
          beforeTree: beforeCommit.tree.oid,
          beforeParents: (beforeCommit.parents?.nodes ?? [])
            .filter(notEmpty)
            .map(node => node.oid),
          afterCommit: afterCommit.oid,
          afterCommittedDate: beforeCommit.committedDate,
          afterTree: afterCommit.tree.oid,
          afterParents: (afterCommit.parents?.nodes ?? [])
            .filter(notEmpty)
            .map(node => node.oid),
        };
      } else {
        return null;
      }
    })
    .filter(notEmpty);
});

/**
 * Internal atom: fetches commit comparison between two commits.
 */
const gitHubCommitComparisonAtom = atomFamily(
  ({base, head}: {base: GitObjectID; head: GitObjectID}) =>
    atom<Promise<CommitComparison | null>>(async get => {
      const client = await get(gitHubClientAtom);
      return client != null ? client.getCommitComparison(base, head) : null;
    }),
  (a, b) => a.base === b.base && a.head === b.head,
);

/**
 * Internal atom: for a given commit in a PR, get its merge base commit
 * with the main branch, as well as all commits on the branch from the
 * base commit to the given head.
 */
const gitHubPullRequestVersionBaseAndCommitsAtom = atomFamily(
  (head: GitObjectID) =>
    atom<
      Promise<{
        baseParent: {oid: GitObjectID; committedDate: DateTime} | null;
        commits: VersionCommit[];
      }>
    >(async get => {
      const base = get(gitHubPullRequestBaseRefAtom);
      if (base == null) {
        return {
          baseParent: null,
          commits: [],
        };
      }

      const commitComparison = await get(gitHubCommitComparisonAtom({base, head}));
      if (commitComparison == null) {
        return {
          baseParent: null,
          commits: [],
        };
      }

      const {mergeBaseCommit, commits} = commitComparison;
      return {
        baseParent: {
          oid: mergeBaseCommit.sha,
          committedDate: mergeBaseCommit.commit.committer.date,
        },
        commits: commits.map(({author, commit, parents, sha}) => ({
          author: author?.login ?? commit.author.name,
          commit: sha,
          committedDate: commit.committer.date,
          title: commit.message.split('\n', 1)[0] ?? '',
          parents: parents.map(({sha}) => sha),
          version: null,
        })),
      };
    }),
  (a, b) => a === b,
);

/**
 *
 * The list of PR versions (each force push creates a new version).
 * Now fully computed in Jotai.
 */
export const gitHubPullRequestVersionsAtom = atom<Promise<Version[]>>(async get => {
  const forcePushes = get(gitHubPullRequestForcePushesAtom);
  const commits = get(gitHubPullRequestCommitsAtom);

  // The "before" commit should represent the head of the latest version of
  // the PR immediately prior to the force push.
  const versions = forcePushes.map(({beforeCommit, beforeCommittedDate}) => ({
    oid: beforeCommit,
    committedDate: beforeCommittedDate,
  }));

  // The latest commit is the head of the latest version. Theoretically, it
  // should always exist.
  const latestCommit = commits[commits.length - 1];
  if (latestCommit != null) {
    versions.push(latestCommit);
  }

  // For now, we special-case Sapling stacks as each version may contain more
  // than one commit.
  const stackedPR = get(stackedPullRequestAtom);
  if (stackedPR.type === 'sapling') {
    const fragments = await get(stackedPullRequestFragmentsAtom);
    if (fragments.length !== stackedPR.body.stack.length) {
      // This is unexpected: bail out.
      return [];
    }
    versions.reverse();

    const index = stackedPR.body.currentStackEntry;
    const parentFragment = fragments[index + 1];

    const saplingStack = stackedPR.body;

    // Prefetch all commits upfront to avoid await in loop
    const allFetchedCommits = await Promise.all(
      commits.map(c => get(gitHubCommitAtom(c.oid))),
    );
    const commitsByOid = new Map<GitObjectID, Commit>();
    allFetchedCommits.forEach((commit, idx) => {
      if (commit != null) {
        commitsByOid.set(commits[idx].oid, commit);
      }
    });

    let cumulativeCommits = 0;
    let previous = commits.length;

    const pr_versions: Version[] = [];
    for (const version of versions) {
      const {numCommits} = saplingStack.stack[saplingStack.currentStackEntry];
      cumulativeCommits += numCommits;

      // We need to separate the commits that were designed to be part of this
      // PR from the ones below in the stack.
      const start = commits.length - cumulativeCommits;
      const commitFragmentsForPRVersion = commits.slice(start, previous);
      previous = start;

      // Get prefetched commits
      const validCommits = commitFragmentsForPRVersion
        .map(c => commitsByOid.get(c.oid))
        .filter(notEmpty);

      const versionCommits: VersionCommit[] = [];
      for (let i = 0; i < forcePushes.length; i++) {
        const f = forcePushes[i];
        if (f.beforeCommit === version.oid) {
          break;
        }
        versionCommits.push({
          author: null,
          commit: f.beforeCommit,
          committedDate: f.beforeCommittedDate,
          title: 'Version ' + (i + 1),
          parents: f.beforeParents,
          version: i + 1,
        });
      }

      validCommits.forEach(c =>
        versionCommits.push({
          author: null,
          commit: c.oid,
          committedDate: c.committedDate,
          title: c.messageHeadline,
          parents: c.parents,
          version: versionCommits.length + 1,
        }),
      );

      let headCommittedDate: DateTime | null = null;
      const headCommit = validCommits[validCommits.length - 1];
      if (headCommit == null) {
        headCommittedDate = latestCommit?.committedDate ?? null;
      } else {
        headCommittedDate = headCommit.committedDate;
      }

      let baseParent: GitObjectID | null = null;
      let baseParentCommittedDate: DateTime | null = null;
      if (parentFragment == null) {
        // the first PR in stack case
        if (validCommits.length > 0) {
          baseParent = validCommits[0].parents[0];
        } else if (forcePushes.length > 0) {
          baseParent = forcePushes[0].beforeParents[0];
          baseParentCommittedDate = forcePushes[0].beforeCommittedDate;
        }
      } else {
        // the not first PR in stack case
        baseParent = parentFragment.headRefOid;
      }

      pr_versions.push({
        headCommit: version.oid,
        headCommittedDate,
        baseParent,
        baseParentCommittedDate,
        commits: versionCommits,
      });
    }

    pr_versions.reverse();
    return pr_versions;
  }

  // Get the base parent and all commits for each version branch.
  const allVersionBaseAndCommits = await Promise.all(
    versions.map(version => get(gitHubPullRequestVersionBaseAndCommitsAtom(version.oid))),
  );

  return allVersionBaseAndCommits
    .map(({baseParent, commits}) => {
      const versionHead = commits[commits.length - 1];
      if (versionHead == null) {
        return null;
      }

      return {
        headCommit: versionHead.commit,
        headCommittedDate: versionHead.committedDate,
        baseParent: baseParent?.oid ?? null,
        baseParentCommittedDate: baseParent?.committedDate ?? null,
        commits,
      };
    })
    .filter(notEmpty);
});

/**
 *
 * The currently selected version index. Defaults to the latest version.
 * This atom is writable - components can set it to change the selected version.
 */
export const gitHubPullRequestSelectedVersionIndexAtom = atom<number>(0);

/**
 *
 * Derived atom that returns the commits for the currently selected version.
 */
export const gitHubPullRequestSelectedVersionCommitsAtom = atom<Promise<VersionCommit[]>>(
  async get => {
    const versions = await get(gitHubPullRequestVersionsAtom);
    const selectedVersionIndex = get(gitHubPullRequestSelectedVersionIndexAtom);
    return versions[selectedVersionIndex]?.commits ?? [];
  },
);

/**
 *
 * Determines if the user is viewing the latest version of the PR.
 * Used to show/hide the "Back to Latest" link.
 */
export const gitHubPullRequestIsViewingLatestAtom = atom<Promise<boolean>>(async get => {
  const versions = await get(gitHubPullRequestVersionsAtom);
  if (versions.length === 0) {
    return true; // Default to true during loading
  }

  const selectedVersionIndex = get(gitHubPullRequestSelectedVersionIndexAtom);
  if (selectedVersionIndex !== versions.length - 1) {
    return false; // Not on the latest version
  }

  const comparableVersions = get(gitHubPullRequestComparableVersionsAtom);
  if (comparableVersions == null) {
    return true; // No explicit comparison selected, assume latest
  }

  // Viewing latest means: no explicit "before" selected, and "after" is the head commit
  const latestVersion = versions[versions.length - 1];
  return (
    comparableVersions.beforeCommitID == null &&
    comparableVersions.afterCommitID === latestVersion.headCommit
  );
});

/**
 *
 * Internal atom that indexes versions by commit ID.
 */
const gitHubPullRequestVersionIndexesByCommitAtom = atom<Promise<Map<GitObjectID, number>>>(
  async get => {
    const versions = await get(gitHubPullRequestVersionsAtom);
    const versionIndexByCommit = new Map<GitObjectID, number>();
    versions.forEach(({commits}, index) => {
      commits.forEach(commit => {
        versionIndexByCommit.set(commit.commit, index);
      });
    });
    return versionIndexByCommit;
  },
);

/**
 *
 * Looks up the version index for a given commit.
 * Used to navigate to the version containing a specific commit.
 */
export const gitHubPullRequestVersionIndexForCommitAtom = atomFamily(
  (commit: GitObjectID) =>
    atom<Promise<number | null>>(async get => {
      const versionIndexesByCommit = await get(gitHubPullRequestVersionIndexesByCommitAtom);
      return versionIndexesByCommit.get(commit) ?? null;
    }),
  (a, b) => a === b,
);

/**
 *
 * Groups review threads by commit ID.
 * Used to count comments per version.
 */
export const gitHubPullRequestThreadsByCommitAtom = atom<
  Map<GitObjectID, GitHubPullRequestReviewThread[]>
>(get => {
  const reviewThreads = get(gitHubPullRequestReviewThreadsAtom);
  const threadsByCommit = new Map<GitObjectID, GitHubPullRequestReviewThread[]>();

  reviewThreads.forEach(thread => {
    // All comments in the thread should have the same original commit as the first
    const firstComment = thread.comments[0];
    const commitOid = firstComment?.originalCommit?.oid;
    if (commitOid != null) {
      const existing = threadsByCommit.get(commitOid) ?? [];
      existing.push(thread);
      threadsByCommit.set(commitOid, existing);
    }
  });

  return threadsByCommit;
});

/**
 *
 * Gets review threads for a specific commit.
 */
export const gitHubPullRequestThreadsForCommitAtom = atomFamily(
  (commitID: GitObjectID) =>
    atom<GitHubPullRequestReviewThread[]>(get => {
      const reviewThreadsByCommit = get(gitHubPullRequestThreadsByCommitAtom);
      return reviewThreadsByCommit.get(commitID) ?? [];
    }),
  (a, b) => a === b,
);

/**
 *
 * Gets review threads for a specific commit and file path.
 */
export const gitHubPullRequestThreadsForCommitFileAtom = atomFamily(
  ({commitID, path}: {commitID: GitObjectID | null; path: string}) =>
    atom<GitHubPullRequestReviewThread[]>(get => {
      if (commitID == null) {
        return [];
      }

      const threadsForCommit = get(gitHubPullRequestThreadsForCommitAtom(commitID));
      return threadsForCommit.filter(thread => {
        // All comments in the thread should have the same path as the first
        const threadPath = thread.comments[0]?.path;
        return threadPath === path;
      });
    }),
  (a, b) => a.commitID === b.commitID && a.path === b.path,
);

/**
 *
 * `DiffSide` refers to the side of the split diff view that the thread appears
 * on. For a given commit, in the context of a pull request, the `Left` side is
 * the base commit and includes threads attached to deletions that would appear
 * in red. The `Right` side is the commit itself and includes threads attached
 * to additions that appear in green or unchanged lines that appear in white.
 */
const gitHubPullRequestThreadsForCommitFileBySideAtom = atomFamily(
  ({commitID, path}: {commitID: GitObjectID | null; path: string}) =>
    atom<ThreadsBySide | null>(get => {
      if (commitID == null) {
        return null;
      }
      const threadsForFile = get(gitHubPullRequestThreadsForCommitFileAtom({commitID, path}));
      // Group threads by their diffSide
      const result: ThreadsBySide = {
        LEFT: [],
        RIGHT: [],
      };
      threadsForFile.forEach(thread => {
        if (thread.diffSide === 'LEFT') {
          result.LEFT.push(thread);
        } else if (thread.diffSide === 'RIGHT') {
          result.RIGHT.push(thread);
        }
      });
      return result;
    }),
  (a, b) => a.commitID === b.commitID && a.path === b.path,
);

/**
 *
 * Get the appropriate threads for each side of the diff for a pull request,
 * depending on what is being compared as "before" and "after".
 */
export const gitHubPullRequestThreadsForDiffFileAtom = atomFamily(
  (path: string) =>
    atom<ThreadsBySide | null>(get => {
      const comparableVersions = get(gitHubPullRequestComparableVersionsAtom);
      if (comparableVersions == null) {
        return null;
      }

      const {beforeCommitID, afterCommitID} = comparableVersions;
      const afterThreads = get(
        gitHubPullRequestThreadsForCommitFileBySideAtom({commitID: afterCommitID, path}),
      );

      // If there is no explicit "before" (i.e., the "after" is being compared
      // against its base), show the "after" threads as they are, according to
      // their original diff sides.
      if (beforeCommitID == null) {
        return afterThreads;
      }

      const beforeThreads = get(
        gitHubPullRequestThreadsForCommitFileBySideAtom({commitID: beforeCommitID, path}),
      );

      // If both "before" and "after" are explicitly selected, then both commits
      // themselves are being shown (i.e., we are comparing two `Right` sides).
      // Therefore, we should display the threads that are attached to the
      // `Right` sides of their respective diffs.
      return {
        LEFT: beforeThreads?.RIGHT ?? [],
        RIGHT: afterThreads?.RIGHT ?? [],
      };
    }),
  (a, b) => a === b,
);

// =============================================================================
// Pull Request Review Threads
// =============================================================================

/**
 * Type for threads organized by diff side.
 * DiffSide.Left = deletions (red), DiffSide.Right = additions/unchanged (green/white)
 */
export type ThreadsBySide = {[key in DiffSide]: GitHubPullRequestReviewThread[]};

/**
 *
 * Get the appropriate threads for each side of the diff.
 * Returns threads for pull request diffs, or null for commit-only views.
 */
export const gitHubThreadsForDiffFileAtom = atomFamily(
  (path: string) =>
    atom<ThreadsBySide | null>(get => {
      const pullRequest = get(gitHubPullRequestAtom);
      if (pullRequest != null) {
        return get(gitHubPullRequestThreadsForDiffFileAtom(path));
      }
      return null;
    }),
  (a, b) => a === b,
);

// =============================================================================
// Line to Position Mapping
// =============================================================================

/**
 * Type for line-to-position mapping per diff side.
 * Maps line numbers to their "position" values for GitHub comments API.
 */
export type LineToPositionBySide =
  | {
      [key in DiffSide]: {[key: number]: number} | null;
    }
  | null;

/**
 *
 * This atomFamily stores the line-to-position mapping for each file.
 * The position value is required when adding comments via the GitHub API.
 */
export const gitHubPullRequestLineToPositionForFileAtom = atomFamily(
  (_path: string) => atom<LineToPositionBySide>(null),
  (a, b) => a === b,
);

// =============================================================================
// Computed Line-to-Position
// =============================================================================

/**
 *
 * For a given commit, returns a map from file path to the diff change for that file.
 * The diff is computed against the commit's base parent (merge base with main branch).
 */
const gitHubPullRequestDiffCommitWithBaseByPathAtom = atomFamily(
  (commitID: GitObjectID) =>
    atom<Promise<Map<string, CommitChange> | null>>(async get => {
      const baseParent = await get(gitHubPullRequestCommitBaseParentAtom(commitID));
      const baseCommitID = baseParent?.oid;
      if (baseCommitID == null) {
        return null;
      }

      const diffWithCommitIDs = await get(
        gitHubDiffForCommitsAtom({baseCommitID, commitID}),
      );
      const diff = diffWithCommitIDs?.diff;
      if (diff == null) {
        return null;
      }

      const diffByPath = new Map<string, CommitChange>();
      diff.forEach(change => diffByPath.set(getPathForChange(change), change));
      return diffByPath;
    }),
  (a, b) => a === b,
);

/**
 *
 * For a given commit and file path, computes the line-to-position mapping.
 * The position value is required when adding comments via the GitHub API.
 *
 * This function:
 * 1. Gets the diff for the file from the commit's canonical diff (vs base parent)
 * 2. Fetches the blobs for before/after (required for lineToPosition computation)
 * 3. Calls the lineToPosition worker to compute the mapping
 */
const gitHubPullRequestLineToPositionForCommitFileAtom = atomFamily(
  ({commitID, path}: {commitID: GitObjectID; path: string}) =>
    atom<Promise<LineToPositionBySide | null>>(async get => {
      const diffsByPath = await get(gitHubPullRequestDiffCommitWithBaseByPathAtom(commitID));
      const diffForPath = diffsByPath?.get(path);
      if (diffForPath == null) {
        return null;
      }

      const entries = getTreeEntriesForChange(diffForPath);
      const oldOID = entries.before?.oid ?? null;
      const newOID = entries.after?.oid ?? null;

      // Before calling the lineToPosition RPC, the Blob for any oid that is
      // passed *must* be persisted to IndexedDB beforehand because our Web
      // Workers are configured to read from IndexedDB but not write.
      await Promise.all([
        oldOID != null ? get(gitHubBlobAtom(oldOID)) : null,
        newOID != null ? get(gitHubBlobAtom(newOID)) : null,
      ]);

      const lineToPosition = await get(lineToPositionAtom({oldOID, newOID}));
      return lineToPosition as LineToPositionBySide;
    }),
  (a, b) => a.commitID === b.commitID && a.path === b.path,
);

/**
 *
 * Computes the line-to-position mapping for a file in the current PR view.
 * The mapping depends on which versions are being compared:
 * - If no explicit "before" (comparing against base), use the "after" mappings directly
 * - If comparing two versions, use the RIGHT side mappings from each version
 */
export const gitHubPullRequestComputedLineToPositionForFileAtom = atomFamily(
  (path: string) =>
    atom<Promise<LineToPositionBySide | null>>(async get => {
      const comparableVersions = get(gitHubPullRequestComparableVersionsAtom);
      if (comparableVersions == null) {
        return null;
      }

      const {beforeCommitID, afterCommitID} = comparableVersions;

      // Handle empty afterCommitID (loading state)
      if (afterCommitID === '') {
        return null;
      }

      const afterLineToPosition = await get(
        gitHubPullRequestLineToPositionForCommitFileAtom({commitID: afterCommitID, path}),
      );

      // If there is no explicit "before" (i.e., the "after" is being compared
      // against its base), directly use the "after" line mappings.
      if (beforeCommitID == null) {
        return afterLineToPosition;
      }

      const beforeLineToPosition = await get(
        gitHubPullRequestLineToPositionForCommitFileAtom({commitID: beforeCommitID, path}),
      );

      // If both "before" and "after" are explicitly selected, then both commits
      // themselves are being shown (i.e., we are comparing two `Right` sides).
      // Therefore, we should use the mappings for the `Right` sides of their
      // respective diffs.
      return {
        [DiffSide.Left]: beforeLineToPosition?.[DiffSide.Right] ?? null,
        [DiffSide.Right]: afterLineToPosition?.[DiffSide.Right] ?? null,
      };
    }),
  (a, b) => a === b,
);

// =============================================================================
// Pull Request Check Runs
// =============================================================================

/**
 * Type for check runs with workflow name included.
 */
export type CheckRun = {
  workflowName: string | undefined;
} & CheckRunFragment;

/**
 *
 * Derived atom that extracts check runs from the current pull request's
 * latest commit's check suites. Each check run includes the workflow name
 * for display purposes.
 */
export const gitHubPullRequestCheckRunsAtom = atom<CheckRun[]>(get => {
  const pullRequest = get(gitHubPullRequestAtom);
  const latestCommit = pullRequest?.commits.nodes?.[0]?.commit;
  const checkSuites = latestCommit?.checkSuites?.nodes ?? [];
  return checkSuites.flatMap(checkSuite => {
    if (checkSuite != null) {
      const {checkRuns, workflowRun} = checkSuite;
      const workflowName = workflowRun?.workflow.name;
      return (
        checkRuns?.nodes
          ?.map(fragment => (fragment != null ? {...fragment, workflowName} : null))
          .filter(notEmpty) ?? []
      );
    } else {
      return [];
    }
  });
});

// =============================================================================
// User Home Page Data
// =============================================================================

/**
 *
 * Async atom that fetches the viewer's home-page PR data.
 * This includes review requests and recent pull requests.
 */
export const gitHubUserHomePageDataAtom = atom<Promise<UserHomePageQueryData | null>>(_get => {
  const token = localStorage.getItem('github.token');
  if (token == null) {
    return Promise.resolve(null);
  }

  // Based on search query for https://github.com/pulls/review-requested
  const reviewRequestedQuery = 'is:open is:pr archived:false review-requested:@me';

  const hostname = localStorage.getItem('github.hostname') ?? 'github.com';
  const graphQLEndpoint = createGraphQLEndpointForHostname(hostname);
  return queryGraphQL<UserHomePageQueryData, UserHomePageQueryVariables>(
    UserHomePageQuery,
    {reviewRequestedQuery},
    createRequestHeaders(token),
    graphQLEndpoint,
  );
});

// =============================================================================
// Pull Requests Search
// =============================================================================

/**
 *
 * Fetches search results for a (labels, states, pagination) tuple.
 * Returns pull requests matching the query criteria.
 */
export const gitHubPullRequestsAtom = atomFamily(
  (input: PullsQueryInput) =>
    atom<Promise<PullsWithPageInfo | null>>(async get => {
      const client = await get(gitHubClientAtom);
      if (client == null) {
        return null;
      }
      return client.getPullRequests(input);
    }),
  (a, b) => {
    // Compare pagination params - check which variant we have
    const aHasFirst = 'first' in a;
    const bHasFirst = 'first' in b;
    if (aHasFirst !== bHasFirst) {
      return false;
    }
    if (aHasFirst && bHasFirst) {
      if (a.first !== b.first || a.after !== b.after) {
        return false;
      }
    } else {
      // Both have 'last' variant
      const aLast = a as {last: number; before?: string};
      const bLast = b as {last: number; before?: string};
      if (aLast.last !== bLast.last || aLast.before !== bLast.before) {
        return false;
      }
    }
    // Compare labels and states
    if (a.labels.length !== b.labels.length) {
      return false;
    }
    if (!a.labels.every((label, i) => label === b.labels[i])) {
      return false;
    }
    if (a.states.length !== b.states.length) {
      return false;
    }
    if (!a.states.every((state, i) => state === b.states[i])) {
      return false;
    }
    return true;
  },
);

// =============================================================================
// Pull Request Pending Review ID
// =============================================================================

/**
 *
 * A PR should have at most a single pending review per user. Any inline
 * comments made will either create a new pending review or be added to the
 * existing one.
 */
export const gitHubPullRequestPendingReviewIDAtom = atom<ID | null>(get => {
  const pullRequest = get(gitHubPullRequestAtom);
  const pendingReview = (pullRequest?.timelineItems?.nodes ?? []).find(
    item => item?.__typename === 'PullRequestReview' && item.state === 'PENDING',
  );
  return (pendingReview as {id: ID} | undefined)?.id ?? null;
});

// =============================================================================
// Pull Request Review Threads
// =============================================================================

/**
 *
 * Extracts and normalizes review threads from the pull request.
 */
export const gitHubPullRequestReviewThreadsAtom = atom<GitHubPullRequestReviewThread[]>(get => {
  const pullRequest = get(gitHubPullRequestAtom);
  return (pullRequest?.reviewThreads.nodes ?? []).filter(notEmpty).map(reviewThread => {
    const {originalLine, diffSide, comments} = reviewThread;
    const normalizedComments = (comments?.nodes ?? [])
      .map(comment => {
        if (comment == null) {
          return null;
        }

        const {id, author, originalCommit, path, state, bodyHTML} = comment;
        const reviewThreadComment = {
          id,
          author: author ?? null,
          originalCommit,
          path,
          state,
          bodyHTML,
        };
        return reviewThreadComment;
      })
      .filter(notEmpty);
    const firstCommentID = normalizedComments[0].id;
    return {
      firstCommentID,
      originalLine,
      diffSide,
      comments: normalizedComments,
    };
  });
});

/**
 *
 * Returns review threads indexed by the ID of their first comment.
 * Used to look up thread information when rendering inline comments.
 */
export const gitHubPullRequestReviewThreadsByFirstCommentIDAtom = atom<{
  [id: ID]: GitHubPullRequestReviewThread;
}>(get => {
  return Object.fromEntries(
    get(gitHubPullRequestReviewThreadsAtom).map(thread => [thread.firstCommentID, thread]),
  );
});

// =============================================================================
// Pull Request Comment Lookup
// =============================================================================

// Note: PullRequestReviewComment type is imported from pullRequestTimelineTypes.ts

/**
 * Internal atom that indexes all review comments by their ID.
 */
const gitHubPullRequestReviewCommentsByIDAtom = atom<Map<ID, PullRequestReviewComment>>(get => {
  const reviewThreads = get(gitHubPullRequestReviewThreadsAtom);
  const commentsByID = new Map<ID, PullRequestReviewComment>();
  reviewThreads.forEach(({originalLine, comments}) => {
    comments.forEach(comment => {
      const {id} = comment;
      if (id != null) {
        commentsByID.set(id, {originalLine, comment});
      }
    });
  });
  return commentsByID;
});

/**
 *
 * Looks up a review comment by its ID. Returns the comment with its
 * original line number for display purposes.
 */
export const gitHubPullRequestCommentForIDAtom = atomFamily(
  (id: ID) =>
    atom<PullRequestReviewComment | null>(get => {
      const commentsByID = get(gitHubPullRequestReviewCommentsByIDAtom);
      return commentsByID.get(id) ?? null;
    }),
  (a, b) => a === b,
);

// =============================================================================
// GitHub Blob
// =============================================================================

/**
 *
 * Fetches a blob by its OID using the GitHub client.
 */
export const gitHubBlobAtom = atomFamily(
  (oid: string) =>
    atom<Promise<Blob | null>>(async get => {
      const client = await get(gitHubClientAtom);
      return client != null ? client.getBlob(oid) : null;
    }),
  (a, b) => a === b,
);

// =============================================================================
// File Contents Delta
// =============================================================================

/**
 * Type for file modification (before/after OIDs and path).
 */
export type FileMod = {
  before: GitObjectID | null;
  after: GitObjectID | null;
  path: string;
};

/**
 * Type for the file contents delta (before/after blobs).
 */
export type FileContentsDelta = {before: Blob | null; after: Blob | null};

/**
 *
 * Derives the before/after blob contents for a file modification.
 * Used for rendering diffs.
 */
export const fileContentsDeltaAtom = atomFamily(
  (mod: FileMod) =>
    atom<Promise<FileContentsDelta>>(async get => {
      const [before, after] = await Promise.all([
        mod.before != null ? get(gitHubBlobAtom(mod.before)) : null,
        mod.after != null ? get(gitHubBlobAtom(mod.after)) : null,
      ]);
      return {before, after};
    }),
  (a, b) => a.before === b.before && a.after === b.after && a.path === b.path,
);

// =============================================================================
// Comment Input State
// =============================================================================

/**
 * Type for pending scroll restore position.
 */
export type PendingScrollRestore = {
  scrollX: number;
  scrollY: number;
} | null;

/**
 * Atom to track scroll position that should be restored after a pull request
 * refresh completes. When a comment is added, the scroll position is saved here
 * before the refresh, and restored after the pull request data updates.
 */
export const pendingScrollRestoreAtom = atom<PendingScrollRestore>(null);

/**
 * Type for the new comment input cell state.
 */
export type NewCommentInputCell = {
  lineNumber: number;
  path: string;
  side: DiffSide;
} | null;

/**
 *
 * Stores which cell (line number, path, side) the user is currently adding a
 * comment to. When null, no comment input is shown.
 */
export const gitHubPullRequestNewCommentInputCellAtom = atom<NewCommentInputCell>(null);

/**
 *
 * Returns whether the new comment input is shown for a specific cell.
 */
export const gitHubPullRequestNewCommentInputShownAtom = atomFamily(
  ({lineNumber, path, side}: {lineNumber: number | null; path: string; side: DiffSide}) =>
    atom<boolean>(get => {
      const cell = get(gitHubPullRequestNewCommentInputCellAtom);
      return (
        cell != null && cell.path === path && cell.lineNumber === lineNumber && cell.side === side
      );
    }),
  (a, b) => a.lineNumber === b.lineNumber && a.path === b.path && a.side === b.side,
);

/**
 *
 * Looks up the "position" value for a line in a file. The position is required
 * by the GitHub API when adding comments to diffs.
 */
export const gitHubPullRequestPositionForLineAtom = atomFamily(
  ({line, path, side}: {line: number; path: string; side: DiffSide}) =>
    atom<number | null>(get => {
      const lineToPosition = get(gitHubPullRequestLineToPositionForFileAtom(path));
      const lineToPositionForSide = lineToPosition?.[side];
      return lineToPositionForSide?.[line] ?? null;
    }),
  (a, b) => a.line === b.line && a.path === b.path && a.side === b.side,
);

/**
 *
 * Determines if a comment can be added to a specific line of a pull request.
 * Requirements:
 * - The line must have a valid "position" value (appears in the canonical diff)
 * - The commit must be part of the latest version (currently exists in the PR)
 * - If comparing versions (beforeCommitID is set), cannot comment on the left side
 *   because that commit is from an older version that may no longer be in the PR
 */
export const gitHubPullRequestCanAddCommentAtom = atomFamily(
  ({lineNumber, path, side}: {lineNumber: number | null; path: string; side: DiffSide}) =>
    atom<Promise<boolean>>(async get => {
      if (lineNumber == null) {
        return false;
      }

      const pullRequest = get(gitHubPullRequestAtom);
      if (pullRequest == null) {
        return false;
      }

      const position = get(gitHubPullRequestPositionForLineAtom({line: lineNumber, path, side}));
      if (position == null) {
        return false;
      }

      const versions = await get(gitHubPullRequestVersionsAtom);
      const selectedVersionIndex = get(gitHubPullRequestSelectedVersionIndexAtom);

      // Must be viewing the latest version to add comments
      if (selectedVersionIndex !== versions.length - 1) {
        return false;
      }

      // If comparing versions (beforeCommitID is set), cannot comment on the left side
      // because that commit is from an older version that may no longer be in the PR
      const comparableVersions = get(gitHubPullRequestComparableVersionsAtom);
      if (comparableVersions?.beforeCommitID != null && side === DiffSide.Left) {
        return false;
      }

      return true;
    }),
  (a, b) => a.lineNumber === b.lineNumber && a.path === b.path && a.side === b.side,
);

// =============================================================================
// Notification Atoms
// =============================================================================

/**
 * A simple notification message to display to the user.
 * Set to null to dismiss the notification.
 */
export type NotificationMessage = {
  type: 'info' | 'warning' | 'error';
  message: string;
} | null;

export const notificationMessageAtom = atom<NotificationMessage>(null);

// =============================================================================
// Auth Error Message
// =============================================================================

/**
 * Stores an error message to display on the login page, typically used when
 * the user's token has expired or been revoked. This is set when a 401 error
 * is encountered and cleared when the user successfully logs in.
 */
export const authErrorMessageAtom = atom<string | null>(null);

// =============================================================================
// Stacked Pull Requests
// =============================================================================

/**
 *
 * Represents the type of stacked PR detected (Sapling, ghstack, or none).
 */
export type StackedPullRequest =
  | {
      type: 'sapling';
      body: SaplingPullRequestBody;
    }
  | {
      type: 'ghstack';
      stack: number[];
    }
  | {
      type: 'no-stack';
    };

/**
 *
 * Parses the pull request body to detect Sapling or ghstack stacks.
 */
export const stackedPullRequestAtom = atom<StackedPullRequest>(get => {
  const pullRequest = get(gitHubPullRequestAtom);
  const body = pullRequest?.body;
  if (body != null) {
    const saplingStack = parseSaplingStackBody(body);
    if (saplingStack != null) {
      return {type: 'sapling', body: saplingStack};
    }

    const ghstack = pullRequestNumbersFromBody(body);
    if (ghstack != null) {
      return {type: 'ghstack', stack: ghstack};
    }
  }

  return {type: 'no-stack'};
});

/**
 *
 * Extracts the PR numbers from the stacked PR info.
 */
const stackedPullRequestNumbersAtom = atom<number[]>(get => {
  const stacked = get(stackedPullRequestAtom);
  switch (stacked.type) {
    case 'no-stack':
      return [];
    case 'sapling': {
      return stacked.body.stack.map(({number}) => number);
    }
    case 'ghstack': {
      return stacked.stack;
    }
  }
});

/**
 *
 * Fetches the stack PR fragments for display.
 */
export const stackedPullRequestFragmentsAtom = atom<Promise<StackPullRequestFragment[]>>(
  async get => {
    const client = await get(gitHubClientAtom);
    const prs = get(stackedPullRequestNumbersAtom);
    if (client == null || prs.length === 0) {
      return [];
    }
    return client.getStackPullRequests(prs);
  },
);

