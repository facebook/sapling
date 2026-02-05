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

import type {
  CheckRunFragment,
  DiffSide,
  LabelFragment,
  UserFragment,
  UserHomePageQueryData,
  UserHomePageQueryVariables,
} from '../generated/graphql';
import type GitHubClient from '../github/GitHubClient';
import type {DiffCommitIDs, DiffWithCommitIDs} from '../github/diffTypes';
import type {
  GitHubPullRequestReviewThread,
  PullRequest,
  PullRequestReviewComment,
} from '../github/pullRequestTimelineTypes';
import type {PullsQueryInput, PullsWithPageInfo} from '../github/pullsTypes';
import type {
  Blob,
  Commit,
  DateTime,
  GitObjectID,
  ID,
  Version,
  VersionCommit,
} from '../github/types';

import {UserHomePageQuery} from '../generated/graphql';
import CachingGitHubClient, {openDatabase} from '../github/CachingGitHubClient';
import GraphQLGitHubClient from '../github/GraphQLGitHubClient';
import {diffCommitWithParent, diffCommits} from '../github/diff';
import {diffVersions} from '../github/diffVersions';
import {createGraphQLEndpointForHostname} from '../github/gitHubCredentials';
import queryGraphQL from '../github/queryGraphQL';
import {atom} from 'jotai';
import {atomFamily} from 'jotai-family';
import {atomWithStorage} from 'jotai/utils';
import {createRequestHeaders} from 'shared/github/auth';
import {notEmpty} from 'shared/utils';

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
    const hostname = localStorage.getItem('github.hostname') ?? 'github.com';
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

// =============================================================================
// Current Commit
// =============================================================================

/**
 * Migrated from: gitHubCurrentCommit in recoil.ts
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
 * Migrated from: gitHubDiffForCurrentCommit in recoil.ts
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
 * Migrated from: ComparableVersions type in recoil.ts
 *
 * When there is no "before" explicitly selected, the view shows the Diff for
 * the selected "after" version compared to its parent.
 */
export type ComparableVersions = {
  beforeCommitID: GitObjectID | null;
  afterCommitID: GitObjectID;
};

/**
 * Migrated from: gitHubPullRequestComparableVersions in recoil.ts
 *
 * Stores the currently selected versions for comparison in a PR.
 * This is a writable atom - the default is computed from versions (done by consumers).
 */
export const gitHubPullRequestComparableVersionsAtom = atom<ComparableVersions | null>(null);

/**
 * Migrated from: gitHubCommit selectorFamily in recoil.ts
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
 * Migrated from: gitHubPullRequestCommitBaseParent selectorFamily in recoil.ts
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
 * Migrated from: gitHubDiffForCommits selectorFamily in recoil.ts
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
 * Migrated from: gitHubPullRequestVersionDiff selector in recoil.ts
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
 * Migrated from: gitHubDiffCommitIDs selector in recoil.ts
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
 * Migrated from: gitHubPullRequestVersions selector in recoil.ts
 *
 * The list of PR versions (each force push creates a new version).
 * This is a complex selector with many dependencies, so during migration
 * it receives its value from Recoil via JotaiRecoilSync.
 */
export const gitHubPullRequestVersionsAtom = atom<Version[]>([]);

/**
 * Migrated from: gitHubPullRequestSelectedVersionIndex atom in recoil.ts
 *
 * The currently selected version index. Defaults to the latest version.
 * This atom is writable - components can set it to change the selected version.
 */
export const gitHubPullRequestSelectedVersionIndexAtom = atom<number>(0);

/**
 * Migrated from: gitHubPullRequestSelectedVersionCommits selector in recoil.ts
 *
 * Derived atom that returns the commits for the currently selected version.
 */
export const gitHubPullRequestSelectedVersionCommitsAtom = atom<VersionCommit[]>(get => {
  const versions = get(gitHubPullRequestVersionsAtom);
  const selectedVersionIndex = get(gitHubPullRequestSelectedVersionIndexAtom);
  return versions[selectedVersionIndex]?.commits ?? [];
});

/**
 * Migrated from: gitHubPullRequestIsViewingLatest selector in recoil.ts
 *
 * Determines if the user is viewing the latest version of the PR.
 * Used to show/hide the "Back to Latest" link.
 */
export const gitHubPullRequestIsViewingLatestAtom = atom<boolean>(get => {
  const versions = get(gitHubPullRequestVersionsAtom);
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
 * Migrated from: gitHubPullRequestVersionIndexesByCommit selector in recoil.ts
 *
 * Internal atom that indexes versions by commit ID.
 */
const gitHubPullRequestVersionIndexesByCommitAtom = atom<Map<GitObjectID, number>>(get => {
  const versions = get(gitHubPullRequestVersionsAtom);
  const versionIndexByCommit = new Map<GitObjectID, number>();
  versions.forEach(({commits}, index) => {
    commits.forEach(commit => {
      versionIndexByCommit.set(commit.commit, index);
    });
  });
  return versionIndexByCommit;
});

/**
 * Migrated from: gitHubPullRequestVersionIndexForCommit selectorFamily in recoil.ts
 *
 * Looks up the version index for a given commit.
 * Used to navigate to the version containing a specific commit.
 */
export const gitHubPullRequestVersionIndexForCommitAtom = atomFamily(
  (commit: GitObjectID) =>
    atom<number | null>(get => {
      const versionIndexesByCommit = get(gitHubPullRequestVersionIndexesByCommitAtom);
      return versionIndexesByCommit.get(commit) ?? null;
    }),
  (a, b) => a === b,
);

/**
 * Migrated from: gitHubPullRequestThreadsByCommit selector in recoil.ts
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
 * Migrated from: gitHubPullRequestThreadsForCommit selectorFamily in recoil.ts
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

// =============================================================================
// Pull Request Review Threads
// =============================================================================

/**
 * Type for threads organized by diff side.
 * DiffSide.Left = deletions (red), DiffSide.Right = additions/unchanged (green/white)
 */
export type ThreadsBySide = {[key in DiffSide]: GitHubPullRequestReviewThread[]};

/**
 * Migrated from: gitHubThreadsForDiffFile selectorFamily in recoil.ts
 *
 * This atomFamily stores the appropriate threads for each side of the diff for a file,
 * depending on what is being compared as "before" and "after".
 *
 * During migration, this receives its value from Recoil via JotaiRecoilSync.
 * The atomFamily pattern allows storing threads per file path.
 */
export const gitHubThreadsForDiffFileAtom = atomFamily(
  (_path: string) => atom<ThreadsBySide | null>(null),
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
 * Migrated from: gitHubPullRequestLineToPositionForFile selectorFamily in recoil.ts
 *
 * This atomFamily stores the line-to-position mapping for each file.
 * The position value is required when adding comments via the GitHub API.
 *
 * During migration, this receives its value from Recoil via useSplitDiffViewData.
 */
export const gitHubPullRequestLineToPositionForFileAtom = atomFamily(
  (_path: string) => atom<LineToPositionBySide>(null),
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
 * Migrated from: gitHubPullRequestCheckRuns selector in recoil.ts
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
 * Migrated from: gitHubUserHomePageData selector in recoil.ts
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
 * Migrated from: gitHubPullRequests selectorFamily in recoil.ts
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
 * Migrated from: gitHubPullRequestPendingReviewID selector in recoil.ts
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
 * Migrated from: gitHubPullRequestReviewThreads selector in recoil.ts
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
 * Migrated from: gitHubPullRequestReviewThreadsByFirstCommentID selector in recoil.ts
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
 * Migrated from: gitHubPullRequestCommentForID selectorFamily in recoil.ts
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
 * Migrated from: gitHubBlob selectorFamily in recoil.ts
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
 * Migrated from: fileContentsDelta selectorFamily in recoil.ts
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
