/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * This file contains Jotai atoms for the ReviewStack application.
 * These atoms are migrated from Recoil and implemented natively in Jotai.
 */

import type {LabelFragment, UserFragment} from '../generated/graphql';
import type GitHubClient from '../github/GitHubClient';
import type {PullRequest} from '../github/pullRequestTimelineTypes';
import type {GitObjectID, ID} from '../github/types';

import CachingGitHubClient, {openDatabase} from '../github/CachingGitHubClient';
import GraphQLGitHubClient from '../github/GraphQLGitHubClient';
import {atom} from 'jotai';
import {atomFamily} from 'jotai-family';
import {atomWithStorage} from 'jotai/utils';

// =============================================================================
// Theme Atoms
// =============================================================================

/**
 * Migrated from: primerColorMode in themeState.ts
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
    const hostname = localStorage.getItem('github.hostname') ?? 'api.github.com';
    const client = new GraphQLGitHubClient(hostname, org, repo, token);
    return new CachingGitHubClient(db, client, org, repo);
  } else {
    return null;
  }
});

// =============================================================================
// Repo Labels
// =============================================================================

/**
 * Migrated from: gitHubRepoLabelsQuery in recoil.ts
 *
 * Search query for filtering repository labels.
 */
export const gitHubRepoLabelsQuery = atom<string>('');

/**
 * Migrated from: gitHubRepoLabels in recoil.ts
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
 * Migrated from: gitHubRepoAssignableUsersQuery in recoil.ts
 *
 * Search query for filtering assignable users.
 */
export const gitHubRepoAssignableUsersQuery = atom<string>('');

/**
 * Migrated from: gitHubRepoAssignableUsers in recoil.ts
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
 * Migrated from: gitHubPullRequestJumpToCommentID in recoil.ts
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
 * Migrated from: gitHubPullRequestLabels in recoil.ts
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
 * Migrated from: gitHubPullRequestReviewers in recoil.ts
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
 * Migrated from: gitHubCommitID in recoil.ts
 *
 * The current commit ID being viewed.
 */
export const gitHubCommitIDAtom = atom<GitObjectID | null>(null);

/**
 * Migrated from: gitHubPullRequestID in recoil.ts
 *
 * The current pull request number being viewed.
 */
export const gitHubPullRequestIDAtom = atom<number | null>(null);

// =============================================================================
// Pull Request
// =============================================================================

/**
 * Migrated from: gitHubPullRequest in recoil.ts
 *
 * The current pull request data. Set when navigating to a PR.
 */
export const gitHubPullRequestAtom = atom<PullRequest | null>(null);

/**
 * Migrated from: gitHubPullRequestViewerDidAuthor in recoil.ts
 *
 * Derived atom that indicates if the current viewer authored the PR.
 * Used to conditionally show edit controls (labels, reviewers, etc.)
 */
export const gitHubPullRequestViewerDidAuthorAtom = atom<boolean>(get => {
  const pullRequest = get(gitHubPullRequestAtom);
  return pullRequest?.viewerDidAuthor ?? false;
});
