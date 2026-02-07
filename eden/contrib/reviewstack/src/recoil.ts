/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type GitHubClient from './github/GitHubClient';
import type {CommitChange, DiffWithCommitIDs} from './github/diffTypes';
import type {
  CommitData,
  PullRequestCommitItem,
  PullRequest,
} from './github/pullRequestTimelineTypes';
import type {CommitComparison} from './github/restApiTypes';
import type {
  Blob,
  Commit,
  DateTime,
  ForcePushEvent,
  GitObjectID,
  Version,
  VersionCommit,
} from './github/types';
import type {LineToPosition} from './lineToPosition';
import type {RecoilValueReadOnly} from 'recoil';

import {lineToPosition} from './diffServiceClient';
import {DiffSide} from './generated/graphql';
import CachingGitHubClient, {openDatabase} from './github/CachingGitHubClient';
import GraphQLGitHubClient from './github/GraphQLGitHubClient';
import {diffCommits} from './github/diff';
import {
  gitHubHostname,
  gitHubTokenPersistence,
} from './github/gitHubCredentials';
import {stackedPullRequest, stackedPullRequestFragments} from './stackState';
import {getPathForChange, getTreeEntriesForChange} from './utils';
import {atom, atomFamily, constSelector, selector, selectorFamily, waitForAll} from 'recoil';
import {notEmpty} from 'shared/utils';

// Internal type - exported from jotai/atoms.ts for component consumers.
type GitHubOrgAndRepo = {
  org: string;
  repo: string;
};

export const gitHubOrgAndRepo = atom<GitHubOrgAndRepo | null>({
  key: 'gitHubOrgAndRepo',
  default: null,
});

export type GitHubPullRequestParams = {
  orgAndRepo: GitHubOrgAndRepo;
  number: number;
};

/** Promise that never settles. */
// eslint-disable-next-line @typescript-eslint/no-empty-function
const never: Promise<PullRequest | null> = new Promise(() => {});

export const gitHubPullRequestForParams = atomFamily<PullRequest | null, GitHubPullRequestParams>({
  key: 'gitHubPullRequestForParams',
  default: selectorFamily({
    key: 'gitHubPullRequestForParams/Default',
    get:
      ({orgAndRepo, number}) =>
      ({get}) => {
        const client = get(gitHubClientForParams(orgAndRepo));
        // If client is null, return a Promise that never settles because we
        // use a return value of `null` to mean "Not found."
        return client != null ? client.getPullRequest(number) : never;
      },
  }),
});

// The GitHubClient may have mutable state for things like an open connection to
// an IDBDatabase.
const ALLOW_MUTABILITY_FOR_GITHUB_CLIENT = true;

const gitHubClientForParams = selectorFamily<GitHubClient | null, GitHubOrgAndRepo>({
  key: 'gitHubClientForParams',
  get:
    ({org, repo}) =>
    ({get}) => {
      const token = get(gitHubTokenPersistence);
      if (token != null) {
        const db = get(databaseConnection);
        const hostname = get(gitHubHostname);
        return createClient(db, token, hostname, org, repo);
      } else {
        return null;
      }
    },
  dangerouslyAllowMutability: ALLOW_MUTABILITY_FOR_GITHUB_CLIENT,
});

export const gitHubPullRequest = atom<PullRequest | null>({
  key: 'gitHubPullRequest',
  default: null,
});

// Internal selector - only used by gitHubPullRequestVersionBaseAndCommits
const gitHubPullRequestBaseRef = selector<GitObjectID | null>({
  key: 'gitHubPullRequestBaseRef',
  get: ({get}) => {
    const pullRequest = get(gitHubPullRequest);
    return pullRequest?.baseRefOid ?? null;
  },
});


// Internal selector - only used by gitHubPullRequestVersions
const gitHubPullRequestCommits = selector<CommitData[]>({
  key: 'gitHubPullRequestCommits',
  get: ({get}) => {
    const pullRequest = get(gitHubPullRequest);
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
  },
});

// Internal selector - only used by gitHubPullRequestVersions
const gitHubPullRequestForcePushes = selector<ForcePushEvent[]>({
  key: 'gitHubPullRequestForcePushes',
  get: ({get}) => {
    const pullRequest = get(gitHubPullRequest);
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
            afterParents: (afterCommit.parents?.nodes ?? []).filter(notEmpty).map(node => node.oid),
          };
        } else {
          return null;
        }
      })
      .filter(notEmpty);
  },
});

/**
 * For a given commit in a PR, get its merge base commit with the main branch,
 * as well as all commits on the branch from the base commit to the given head.
 * Used primarily to construct a Version with all of its associated commits.
 * Internal - only used by gitHubPullRequestVersions and gitHubPullRequestCommitBaseParent.
 */
const gitHubPullRequestVersionBaseAndCommits = selectorFamily<
  {
    baseParent: {oid: GitObjectID; committedDate: DateTime} | null;
    commits: VersionCommit[];
  },
  GitObjectID
>({
  key: 'gitHubPullRequestVersionBaseAndCommits',
  get:
    head =>
    ({get}) => {
      const base = get(gitHubPullRequestBaseRef);
      if (base == null) {
        return {
          baseParent: null,
          commits: [],
        };
      }

      const commitComparison = get(gitHubCommitComparison({base, head}));
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
    },
});

/**
 * For a given commit in a PR, get its merge base commit with the main branch.
 * Used to identify the appropriate base for comparison when generating diffs
 * across versions.
 * Internal - only used by gitHubPullRequestDiffCommitWithBaseByPath.
 */
const gitHubPullRequestCommitBaseParent = selectorFamily<
  {oid: GitObjectID; committedDate: DateTime} | null,
  GitObjectID
>({
  key: 'gitHubPullRequestCommitBaseParent',
  get:
    commitID =>
    ({get}) => {
      const commitComparison = get(gitHubPullRequestVersionBaseAndCommits(commitID));
      return commitComparison?.baseParent;
    },
});

export const gitHubPullRequestVersions = selector<Version[]>({
  key: 'gitHubPullRequestVersions',
  get: ({get}) => {
    const [forcePushes, commits] = get(
      waitForAll([gitHubPullRequestForcePushes, gitHubPullRequestCommits]),
    );

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
    const stackedPR = get(stackedPullRequest);
    if (stackedPR.type === 'sapling') {
      const fragments = get(stackedPullRequestFragments);
      if (fragments.length !== stackedPR.body.stack.length) {
        // This is unexpected: bail out.
        return [];
      }
      versions.reverse();

      const index = stackedPR.body.currentStackEntry;
      const parentFragment = fragments[index + 1];

      const saplingStack = stackedPR.body;

      let cumulativeCommits = 0;
      let previous = commits.length;

      const pr_versions = versions.map(version => {
        const {numCommits} = saplingStack.stack[saplingStack.currentStackEntry];
        cumulativeCommits += numCommits;

        // We need to separate the commits that were designed to be part of this
        // PR from the ones below in the stack.
        const start = commits.length - cumulativeCommits;
        const commitFragmentsForPRVersion = commits.slice(start, previous);
        previous = start;

        // Find gitHubCommit() for each.
        const commitsForPRVersion = get(
          waitForAll(commitFragmentsForPRVersion.map(c => gitHubCommit(c.oid))),
        ) as Commit[];

        const versionCommits = [];
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

        commitsForPRVersion.forEach(c =>
          versionCommits.push({
            author: null,
            commit: c.oid,
            committedDate: c.committedDate,
            title: c.messageHeadline,
            parents: c.parents,
            version: versionCommits.length + 1,
          }),
        );

        let headCommittedDate = null;
        const headCommit = commitsForPRVersion[commitsForPRVersion.length - 1];
        if (headCommit == null) {
          headCommittedDate = latestCommit.committedDate;
        } else {
          headCommittedDate = headCommit.committedDate;
        }

        let baseParent = null;
        let baseParentCommittedDate = null;
        if (parentFragment == null) {
          // the first PR in stack case
          if (commitsForPRVersion.length > 0) {
            baseParent = commitsForPRVersion[0].parents[0];
          } else if (forcePushes.length > 0) {
            baseParent = forcePushes[0].beforeParents[0];
            baseParentCommittedDate = forcePushes[0].beforeCommittedDate;
          }
        } else {
          // the not first PR in stack case
          baseParent = parentFragment.headRefOid;
        }

        return {
          headCommit: version.oid,
          headCommittedDate,
          baseParent,
          baseParentCommittedDate,
          commits: versionCommits,
        };
      });

      pr_versions.reverse();
      return pr_versions;
    }

    // Get the base parent and all commits for each version branch.
    const allVersionBaseAndCommits = get(
      waitForAll(versions.map(version => gitHubPullRequestVersionBaseAndCommits(version.oid))),
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
  },
});

export const gitHubPullRequestSelectedVersionIndex = atom<number>({
  key: 'gitHubPullRequestSelectedVersionIndex',
  default: selector<number>({
    key: 'gitHubPullRequestSelectedVersionIndex/default',
    get: ({get}) => {
      const versions = get(gitHubPullRequestVersions);

      if (versions.length === 0) {
        // Return 0 when no versions available yet (loading state).
        // Dependent selectors should handle this gracefully.
        return 0;
      }

      return versions.length - 1;
    },
  }),
});


/**
 * When there is no "before" explicitly selected, the view shows the Diff for
 * the selected "after" version compared to its parent.
 * Internal type - exported from jotai/atoms.ts for component consumers.
 */
type ComparableVersions = {
  beforeCommitID: GitObjectID | null;
  afterCommitID: GitObjectID;
};

export const gitHubPullRequestComparableVersions = atom<ComparableVersions>({
  key: 'gitHubPullRequestComparableVersions',
  default: selector<ComparableVersions>({
    key: 'gitHubPullRequestComparableVersions/default',
    get: ({get}) => {
      const [versions, selectedVersionIndex] = get(
        waitForAll([gitHubPullRequestVersions, gitHubPullRequestSelectedVersionIndex]),
      );
      const version = versions[selectedVersionIndex];

      // Handle loading state when versions aren't available yet
      if (version == null) {
        return {
          beforeCommitID: null,
          afterCommitID: '' as GitObjectID,
        };
      }

      return {
        beforeCommitID: version.baseParent,
        afterCommitID: version.headCommit,
      };
    },
  }),
});



const gitHubPullRequestDiffCommitWithBaseByPath = selectorFamily<
  Map<string, CommitChange> | null,
  GitObjectID
>({
  key: 'gitHubPullRequestDiffCommitWithBaseByPath',
  get:
    commitID =>
    ({get}) => {
      const baseCommitID = get(gitHubPullRequestCommitBaseParent(commitID))?.oid;
      if (baseCommitID == null) {
        return null;
      }

      const diff = get(gitHubDiffForCommits({baseCommitID, commitID}))?.diff;
      if (diff == null) {
        return null;
      }

      const diffByPath = new Map();
      diff.forEach(change => diffByPath.set(getPathForChange(change), change));
      return diffByPath;
    },
});

// Internal selector - used by gitHubPullRequestVersions and gitHubDiffForCommits
const gitHubCommit = selectorFamily<Commit | null, GitObjectID>({
  key: 'gitHubCommit',
  get:
    (oid: GitObjectID) =>
    ({get}) => {
      const client = get(gitHubClient);
      return client != null ? client.getCommit(oid) : null;
    },
});

// Internal selector - used by gitHubPullRequestVersionBaseAndCommits
const gitHubCommitComparison = selectorFamily<
  CommitComparison | null,
  {base: GitObjectID; head: GitObjectID}
>({
  key: 'gitHubCommitComparison',
  get:
    ({base, head}) =>
    ({get}) => {
      const client = get(gitHubClient);
      return client != null ? client.getCommitComparison(base, head) : null;
    },
});


// Internal selector - used by gitHubPullRequestDiffCommitWithBaseByPath
const gitHubDiffForCommits = selectorFamily<
  DiffWithCommitIDs | null,
  {baseCommitID: GitObjectID; commitID: GitObjectID}
>({
  key: 'gitHubDiffForCommits',
  get:
    ({baseCommitID, commitID}) =>
    ({get}) => {
      const [client, baseCommit, commit] = get(
        waitForAll([gitHubClient, gitHubCommit(baseCommitID), gitHubCommit(commitID)]),
      );
      return client != null && baseCommit != null && commit != null
        ? diffCommits(baseCommit, commit, client)
        : null;
    },
});


export const gitHubClient = selector<GitHubClient | null>({
  key: 'gitHubClient',
  get: ({get}) => {
    const [token, orgAndRepo] = get(waitForAll([gitHubTokenPersistence, gitHubOrgAndRepo]));
    if (token != null && orgAndRepo != null) {
      const {org, repo} = orgAndRepo;
      const db = get(databaseConnection);
      const hostname = get(gitHubHostname);
      return createClient(db, token, hostname, org, repo);
    } else {
      return null;
    }
  },
  dangerouslyAllowMutability: ALLOW_MUTABILITY_FOR_GITHUB_CLIENT,
});

/**
 * The entire window is designed to share one IDBDatabase connection.
 *
 * Will return an error if the value of this selector is read before
 * gitHubTokenPersistence is set.
 */
const databaseConnection = selector<IDBDatabase>({
  key: 'databaseConnection',
  get: ({get}) => {
    const token = get(gitHubTokenPersistence);
    // We should never try to create/open an IndexedDB before the token is set.
    return token != null
      ? openDatabase()
      : Promise.reject('invariant violation: tried to create DB for unauthenticated user');
  },
  dangerouslyAllowMutability: ALLOW_MUTABILITY_FOR_GITHUB_CLIENT,
});

const gitHubBlob = selectorFamily<Blob | null, string>({
  key: 'gitHubBlob',
  get:
    (oid: string) =>
    ({get}) => {
      const client = get(gitHubClient);
      return client != null ? client.getBlob(oid) : null;
    },
});

export const nullAtom: RecoilValueReadOnly<null> = constSelector(null);

/**
 * GitHub comments are attached to diffs and not commits, with placement
 * described using "position". In the context of a pull request, a commit has a
 * canonical diff, in which it is compared against its base parent (not direct
 * parent). We use this diff to generate the line-to-position mapping for a
 * given commit.
 */
const gitHubPullRequestLineToPositionForCommitFile = selectorFamily<
  LineToPosition | null,
  {commitID: GitObjectID; path: string}
>({
  key: 'gitHubPullRequestLineToPositionForCommitFile',
  get:
    ({commitID, path}) =>
    ({get}) => {
      const diffsByPath = get(gitHubPullRequestDiffCommitWithBaseByPath(commitID));
      const diffForPath = diffsByPath?.get(path);
      if (diffForPath == null) {
        return null;
      }

      const entries = getTreeEntriesForChange(diffForPath);
      const params = {
        oldOID: entries.before?.oid ?? null,
        newOID: entries.after?.oid ?? null,
      };

      // Before calling the lineToPosition RPC, the Blob for any oid that is
      // passed *must* be persisted to IndexedDB beforehand because our Web
      // Workers are configured to read from IndexedDB but not write.
      get(
        waitForAll(
          [
            params.oldOID ? gitHubBlob(params.oldOID) : null,
            params.newOID ? gitHubBlob(params.newOID) : null,
          ].filter(notEmpty),
        ),
      );
      return get(lineToPosition(params));
    },
});

export const gitHubPullRequestLineToPositionForFile = selectorFamily<
  {[key in DiffSide]: {[key: number]: number} | null} | null,
  string
>({
  key: 'gitHubPullRequestLineToPositionForFile',
  get:
    path =>
    ({get}) => {
      const {beforeCommitID, afterCommitID} = get(gitHubPullRequestComparableVersions);
      const afterLineToPosition = get(
        gitHubPullRequestLineToPositionForCommitFile({commitID: afterCommitID, path}),
      );

      // If there is no explicit "before" (i.e., the "after" is being compared
      // against its base), directly use the "after" line mappings.
      if (beforeCommitID == null) {
        return afterLineToPosition;
      }

      const beforeLineToPosition = get(
        gitHubPullRequestLineToPositionForCommitFile({commitID: beforeCommitID, path}),
      );

      // If both "before" and "after" are explicitly selected, then both commits
      // themselves are being shown (i.e., we are comparing two `Right` sides).
      // Therefore, we should use the mappings for the `Right` sides of their
      // respective diffs.
      return {
        [DiffSide.Left]: beforeLineToPosition?.[DiffSide.Right] ?? null,
        [DiffSide.Right]: afterLineToPosition?.[DiffSide.Right] ?? null,
      };
    },
});

function createClient(
  db: IDBDatabase,
  token: string,
  hostname: string,
  organization: string,
  repository: string,
): GitHubClient | null {
  const client = new GraphQLGitHubClient(hostname, organization, repository, token);
  return new CachingGitHubClient(db, client, organization, repository);
}
