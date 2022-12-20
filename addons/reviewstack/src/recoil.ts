/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  CheckRunFragment,
  LabelFragment,
  UserFragment,
  UserHomePageQueryData,
  UserHomePageQueryVariables,
} from './generated/graphql';
import type GitHubClient from './github/GitHubClient';
import type {CommitChange, DiffCommitIDs, DiffWithCommitIDs} from './github/diffTypes';
import type {
  CommitData,
  PullRequestCommitItem,
  PullRequestReviewComment,
  PullRequestReviewItem,
  GitHubPullRequestReviewThread,
  PullRequest,
} from './github/pullRequestTimelineTypes';
import type {PullsQueryInput, PullsWithPageInfo} from './github/pullsTypes';
import type {CommitComparison} from './github/restApiTypes';
import type {
  Blob,
  Commit,
  DateTime,
  ForcePushEvent,
  GitObjectID,
  ID,
  Version,
  VersionCommit,
} from './github/types';
import type {LineToPosition} from './lineToPosition';
import type {SaplingPullRequestBody} from './saplingStack';
import type {RecoilValueReadOnly} from 'recoil';

import {lineToPosition} from './diffServiceClient';
import {DiffSide, PullRequestReviewState, UserHomePageQuery} from './generated/graphql';
import CachingGitHubClient, {openDatabase} from './github/CachingGitHubClient';
import GraphQLGitHubClient from './github/GraphQLGitHubClient';
import {diffCommits, diffCommitWithParent} from './github/diff';
import {diffVersions} from './github/diffVersions';
import {
  gitHubGraphQLEndpoint,
  gitHubHostname,
  gitHubTokenPersistence,
  gitHubUsername,
} from './github/gitHubCredentials';
import queryGraphQL from './github/queryGraphQL';
import {stackedPullRequest, stackedPullRequestFragments} from './stackState';
import {getPathForChange, getTreeEntriesForChange, groupBy, groupByDiffSide} from './utils';
import {atom, atomFamily, constSelector, selector, selectorFamily, waitForAll} from 'recoil';
import {createRequestHeaders} from 'shared/github/auth';
import {notEmpty} from 'shared/utils';

export type GitHubOrgAndRepo = {
  org: string;
  repo: string;
};

export const gitHubOrgAndRepo = atom<GitHubOrgAndRepo | null>({
  key: 'gitHubOrgAndRepo',
  default: null,
});

export const gitHubCommitID = atom<GitObjectID | null>({
  key: 'gitHubCommitID',
  default: null,
});

export const gitHubPullRequestID = atom<number | null>({
  key: 'gitHubPullRequestID',
  default: null,
});

export const gitHubCurrentCommit = selector<Commit | null>({
  key: 'gitHubCurrentCommit',
  get: ({get}) => {
    const client = get(gitHubClient);
    const oid = get(gitHubCommitID);
    if (client != null && oid != null) {
      return client.getCommit(oid);
    } else {
      return null;
    }
  },
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

export const gitHubPullRequestViewerDidAuthor = selector<boolean>({
  key: 'gitHubPullRequestViewerDidAuthor',
  get: ({get}) => {
    const pullRequest = get(gitHubPullRequest);
    return pullRequest?.viewerDidAuthor ?? false;
  },
});

export const gitHubPullRequestBaseRef = selector<GitObjectID | null>({
  key: 'gitHubPullRequestBaseRef',
  get: ({get}) => {
    const pullRequest = get(gitHubPullRequest);
    return pullRequest?.baseRefOid ?? null;
  },
});

/**
 * A PR should have at most a single pending review per user. Any inline
 * comments made will either create a new pending review or be added to the
 * existing one.
 */
export const gitHubPullRequestPendingReviewID = selector<ID | null>({
  key: 'gitHubPullRequestPendingReviewID',
  get: ({get}) => {
    const pullRequest = get(gitHubPullRequest);
    const pendingReview = (pullRequest?.timelineItems?.nodes ?? []).find(
      item =>
        item?.__typename === 'PullRequestReview' && item.state === PullRequestReviewState.Pending,
    ) as PullRequestReviewItem;
    return pendingReview?.id ?? null;
  },
});

export const gitHubPullRequestCommits = selector<CommitData[]>({
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

export const gitHubPullRequestForcePushes = selector<ForcePushEvent[]>({
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
 */
export const gitHubPullRequestVersionBaseAndCommits = selectorFamily<
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
        })),
      };
    },
});

/**
 * For a given commit in a PR, get its merge base commit with the main branch.
 * Used to identify the appropriate base for comparison when generating diffs
 * across versions.
 */
export const gitHubPullRequestCommitBaseParent = selectorFamily<
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

/**
 * If we are at the bottom PR in a Sapling stack, we can generally treat it
 * like any other pull request.
 */
function isBottomOfSaplingStack(saplingStack: SaplingPullRequestBody): boolean {
  const index = saplingStack.currentStackEntry;
  // saplingStack.stack has the top of the stack at the front of the array.
  return index === saplingStack.stack.length - 1;
}

export const gitHubPullRequestVersions = selector<Version[]>({
  key: 'gitHubPullRequestVersions',
  get: ({get}) => {
    const [forcePushes, commits] = get(
      waitForAll([gitHubPullRequestForcePushes, gitHubPullRequestCommits]),
    );

    // For now, we special-case Sapling and ignore versions for the moment.
    const stackedPR = get(stackedPullRequest);
    if (stackedPR.type === 'sapling' && !isBottomOfSaplingStack(stackedPR.body)) {
      const fragments = get(stackedPullRequestFragments);
      if (fragments.length !== stackedPR.body.stack.length) {
        // This is unexpected: bail out.
        return [];
      }

      const index = stackedPR.body.currentStackEntry;
      const parentFragment = fragments[index + 1];

      const saplingStack = stackedPR.body;
      const {numCommits} = saplingStack.stack[saplingStack.currentStackEntry];
      // We need to separate the commits that were designed to be part of this
      // PR from the ones below in the stack.
      const commitFragmentsForPR = commits.slice(commits.length - numCommits);

      // Find gitHubCommit() for each.
      const commitsForPR = get(
        waitForAll(commitFragmentsForPR.map(c => gitHubCommit(c.oid))),
      ) as Commit[];
      const versionCommits = commitsForPR.map(c => ({
        author: null,
        commit: c.oid,
        committedDate: c.committedDate,
        title: c.messageHeadline,
        parents: c.parents,
      }));

      const headCommit = commitsForPR[commitsForPR.length - 1];
      return [
        {
          headCommit: headCommit.oid,
          headCommittedDate: headCommit.committedDate,
          baseParent: parentFragment.headRefOid,
          baseParentCommittedDate: null,
          commits: versionCommits,
        },
      ];
    }

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

const gitHubPullRequestVersionIndexesByCommit = selector<Map<GitObjectID, number>>({
  key: 'gitHubPullRequestVersionIndexesByCommit',
  get: ({get}) => {
    const versions = get(gitHubPullRequestVersions);
    const versionIndexByCommit = new Map();
    versions.forEach(({commits}, index) => {
      commits.forEach(commit => {
        versionIndexByCommit.set(commit.commit, index);
      });
    });
    return versionIndexByCommit;
  },
});

export const gitHubPullRequestVersionIndexForCommit = selectorFamily<number | null, GitObjectID>({
  key: 'gitHubPullRequestVersionIndexForCommit',
  get:
    commit =>
    ({get}) => {
      const versionIndexesByCommit = get(gitHubPullRequestVersionIndexesByCommit);
      return versionIndexesByCommit.get(commit) ?? null;
    },
});

export const gitHubPullRequestReviewThreads = selector<GitHubPullRequestReviewThread[]>({
  key: 'gitHubPullRequestReviewThreads',
  get: ({get}) => {
    const pullRequest = get(gitHubPullRequest);
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
  },
});

export const gitHubPullRequestReviewThreadsByFirstCommentID = selector<{
  [id: ID]: GitHubPullRequestReviewThread;
}>({
  key: 'gitHubPullRequestReviewThreadsByFirstCommentID',
  get: ({get}) => {
    return Object.fromEntries(
      get(gitHubPullRequestReviewThreads).map(thread => [thread.firstCommentID, thread]),
    );
  },
});

const gitHubPullRequestReviewCommentsByID = selector<Map<ID, PullRequestReviewComment>>({
  key: 'gitHubPullRequestReviewCommentsByID',
  get: ({get}) => {
    const reviewThreads = get(gitHubPullRequestReviewThreads);
    const commentsByID = new Map();
    reviewThreads.forEach(({originalLine, comments}) => {
      comments.forEach(comment => {
        const {id} = comment;
        if (id != null) {
          commentsByID.set(id, {originalLine, comment});
        }
      });
    });
    return commentsByID;
  },
});

export const gitHubPullRequestCommentForID = selectorFamily<PullRequestReviewComment | null, ID>({
  key: 'gitHubPullRequestCommentForID',
  get:
    id =>
    ({get}) => {
      const commentsByID = get(gitHubPullRequestReviewCommentsByID);
      return commentsByID.get(id) ?? null;
    },
});

export const gitHubPullRequestJumpToCommentID = atomFamily<boolean, ID>({
  key: 'gitHubPullRequestJumpToCommentID',
  default: false,
});

export const gitHubPullRequestNewCommentInputCell = atom<{
  lineNumber: number;
  path: string;
  side: DiffSide;
} | null>({
  key: 'gitHubPullRequestNewCommentInputCell',
  default: null,
});

const gitHubPullRequestNewCommentInputShownForPath = selectorFamily<boolean, string>({
  key: 'gitHubPullRequestNewCommentInputShownForPath',
  get:
    path =>
    ({get}) => {
      const cell = get(gitHubPullRequestNewCommentInputCell);
      return cell?.path === path;
    },
  cachePolicy_UNSTABLE: {eviction: 'most-recent'},
});

export const gitHubPullRequestNewCommentInputShown = selectorFamily<
  boolean,
  {
    lineNumber: number | null;
    path: string;
    side: DiffSide;
  }
>({
  key: 'gitHubPullRequestNewCommentInputShown',
  get:
    ({lineNumber, path, side}) =>
    ({get}) => {
      const shownForPath = get(gitHubPullRequestNewCommentInputShownForPath(path));
      if (!shownForPath) {
        return false;
      }

      const cell = get(gitHubPullRequestNewCommentInputCell);
      return cell?.lineNumber === lineNumber && cell?.side === side;
    },
  cachePolicy_UNSTABLE: {eviction: 'most-recent'},
});

export type NewCommentInputCallbacks = {
  onShowNewCommentInput: (event: React.MouseEvent<HTMLTableElement>) => void;
  onResetNewCommentInput: () => void;
};

export const gitHubPullRequestNewCommentInputCallbacks = selector<NewCommentInputCallbacks>({
  key: 'gitHubPullRequestNewCommentInputCallbacks',
  get: ({getCallback}) => {
    const onShowNewCommentInput = getCallback(
      ({set, snapshot}) =>
        (event: React.MouseEvent<HTMLTableElement>) => {
          const {target} = event;
          if (!(target instanceof HTMLTableCellElement)) {
            return;
          }

          const {lineNumber: _lineNumber, path, side: _side} = target.dataset;
          if (_lineNumber == null || path == null || _side == null) {
            return;
          }

          const lineNumber = parseInt(_lineNumber, 10);
          const side =
            _side === DiffSide.Left
              ? DiffSide.Left
              : _side === DiffSide.Right
              ? DiffSide.Right
              : null;
          if (isNaN(lineNumber) || side == null) {
            return;
          }

          const canAddCommentLoadable = snapshot.getLoadable(
            gitHubPullRequestCanAddComment({lineNumber, path, side}),
          );
          if (canAddCommentLoadable.state !== 'hasValue' || !canAddCommentLoadable.contents) {
            return;
          }

          set(gitHubPullRequestNewCommentInputCell, {path, lineNumber, side});
        },
    );
    const onResetNewCommentInput = getCallback(({reset}) => () => {
      reset(gitHubPullRequestNewCommentInputCell);
    });

    return {onShowNewCommentInput, onResetNewCommentInput};
  },
});

export const gitHubDiffNewCommentInputCallbacks = selector<NewCommentInputCallbacks | null>({
  key: 'gitHubDiffNewCommentInputCallbacks',
  get: ({get}) => {
    const pullRequest = get(gitHubPullRequest);
    if (pullRequest != null) {
      return get(gitHubPullRequestNewCommentInputCallbacks);
    }
    return null;
  },
});

/**
 * In order to add a comment to a line of a pull request commit:
 * - The line must have a valid "position" value (i.e., the line must appear in
 *   the canonical diff for the commit, compared against its base parent).
 * - The commit must be part of the latest version (i.e., the commit must
 *   currently exist in the pull request).
 */
export const gitHubPullRequestCanAddComment = selectorFamily<
  boolean,
  {lineNumber: number | null; path: string; side: DiffSide}
>({
  key: 'gitHubPullRequestCanAddComment',
  get:
    ({lineNumber, path, side}) =>
    ({get}) => {
      const pullRequest = get(gitHubPullRequest);
      if (pullRequest == null || lineNumber == null) {
        return false;
      }

      const [position, versions, selectedVersionIndex] = get(
        waitForAll([
          gitHubPullRequestPositionForLine({line: lineNumber, path, side}),
          gitHubPullRequestVersions,
          gitHubPullRequestSelectedVersionIndex,
        ]),
      );
      return position != null && selectedVersionIndex === versions.length - 1;
    },
});

export const gitHubPullRequestThreadsByCommit = selector<
  Map<GitObjectID, GitHubPullRequestReviewThread[]>
>({
  key: 'gitHubPullRequestThreadsByCommit',
  get: ({get}) => {
    const reviewThreads = get(gitHubPullRequestReviewThreads);
    return groupBy(reviewThreads, thread => {
      // All comments in the thread should have the same original commit as the first
      const firstComment = thread.comments[0];
      return firstComment?.originalCommit?.oid ?? null;
    });
  },
});

export const gitHubPullRequestThreadsForCommit = selectorFamily<
  GitHubPullRequestReviewThread[],
  GitObjectID
>({
  key: 'gitHubPullRequestThreadsForCommit',
  get:
    (commitID: GitObjectID) =>
    ({get}) => {
      const reviewThreadsByCommit = get(gitHubPullRequestThreadsByCommit);
      return reviewThreadsByCommit.get(commitID) ?? [];
    },
});

export const gitHubPullRequestThreadsForCommitFile = selectorFamily<
  GitHubPullRequestReviewThread[],
  {commitID: GitObjectID | null; path: string}
>({
  key: 'gitHubPullRequestThreadsForCommitFile',
  get:
    ({commitID, path}) =>
    ({get}) => {
      if (commitID == null) {
        return [];
      }

      const threadsForCommit = get(gitHubPullRequestThreadsForCommit(commitID));
      return threadsForCommit.filter(thread => {
        // All comments in the thread should have the same path as the first
        const threadPath = thread.comments[0]?.path;
        return threadPath === path;
      });
    },
});

/**
 * `DiffSide` refers to the side of the split diff view that the thread appears
 * on. For a given commit, in the context of a pull request, the `Left` side is
 * the base commit and includes threads attached to deletions that would appear
 * in red. The `Right` side is the commit itself and includes threads attached
 * to additions that appear in green or unchanged lines that appear in white.
 */
const gitHubPullRequestThreadsForCommitFileBySide = selectorFamily<
  {[key in DiffSide]: GitHubPullRequestReviewThread[]} | null,
  {commitID: GitObjectID | null; path: string}
>({
  key: 'gitHubPullRequestThreadsForCommitFileBySide',
  get:
    ({commitID, path}) =>
    ({get}) => {
      if (commitID == null) {
        return null;
      }
      const threadsForFile = get(gitHubPullRequestThreadsForCommitFile({commitID, path}));
      return groupByDiffSide(threadsForFile, thread => thread.diffSide);
    },
});

/**
 * Get the appropriate threads for each side of the diff for a pull request,
 * depending on what is being compared as "before" and "after".
 */
export const gitHubPullRequestThreadsForDiffFile = selectorFamily<
  {[key in DiffSide]: GitHubPullRequestReviewThread[]} | null,
  string
>({
  key: 'gitHubPullRequestThreadsForDiffFile',
  get:
    path =>
    ({get}) => {
      const {beforeCommitID, afterCommitID} = get(gitHubPullRequestComparableVersions);
      const afterThreads = get(
        gitHubPullRequestThreadsForCommitFileBySide({commitID: afterCommitID, path}),
      );

      // If there is no explicit "before" (i.e., the "after" is being compared
      // against its base), show the "after" threads as they are, according to
      // their original diff sides.
      if (beforeCommitID == null) {
        return afterThreads;
      }

      const beforeThreads = get(
        gitHubPullRequestThreadsForCommitFileBySide({commitID: beforeCommitID, path}),
      );

      // If both "before" and "after" are explicitly selected, then both commits
      // themselves are being shown (i.e., we are comparing two `Right` sides).
      // Therefore, we should display the threads that are attached to the
      // `Right` sides of their respective diffs.
      return {
        [DiffSide.Left]: beforeThreads?.[DiffSide.Right] ?? [],
        [DiffSide.Right]: afterThreads?.[DiffSide.Right] ?? [],
      };
    },
});

/**
 * Get the appropriate threads for each side of the diff for a pull request,
 * depending on what is being compared as "before" and "after".
 */
export const gitHubThreadsForDiffFile = selectorFamily<
  {[key in DiffSide]: GitHubPullRequestReviewThread[]} | null,
  string
>({
  key: 'gitHubThreadsForDiffFile',
  get:
    path =>
    ({get}) => {
      const pullRequest = get(gitHubPullRequest);
      if (pullRequest != null) {
        return get(gitHubPullRequestThreadsForDiffFile(path));
      }

      return null;
    },
});

export const gitHubPullRequestSelectedVersionIndex = atom<number>({
  key: 'gitHubPullRequestSelectedVersionIndex',
  default: selector<number>({
    key: 'gitHubPullRequestSelectedVersionIndex/default',
    get: ({get}) => {
      const versions = get(gitHubPullRequestVersions);

      if (versions.length === 0) {
        throw new Error('no versions available for the pull request');
      }

      return versions.length - 1;
    },
  }),
});

export const gitHubPullRequestIsViewingLatest = selector<boolean>({
  key: 'gitHubPullRequestIsViewingLatest',
  get: ({get}) => {
    const [versions, selectedVersionIndex, comparableVersions] = get(
      waitForAll([
        gitHubPullRequestVersions,
        gitHubPullRequestSelectedVersionIndex,
        gitHubPullRequestComparableVersions,
      ]),
    );
    const {beforeCommitID, afterCommitID} = comparableVersions;
    const latestVersionIndex = versions.length - 1;
    const latestVersion = versions[latestVersionIndex];
    const isLatestVersion = selectedVersionIndex === latestVersionIndex;
    const isLatestCommit = beforeCommitID == null && afterCommitID == latestVersion.headCommit;

    return isLatestVersion && isLatestCommit;
  },
});

/**
 * When there is no "before" explicitly selected, the view shows the Diff for
 * the selected "after" version compared to its parent.
 */
export type ComparableVersions = {
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
      const latestCommit = versions[selectedVersionIndex].headCommit;

      return {
        beforeCommitID: null,
        afterCommitID: latestCommit,
      };
    },
  }),
});

export const gitHubPullRequestSelectedVersionCommits = selector<VersionCommit[]>({
  key: 'gitHubPullRequestSelectedVersionCommits',
  get: ({get}) => {
    const [versions, selectedVersionIndex] = get(
      waitForAll([gitHubPullRequestVersions, gitHubPullRequestSelectedVersionIndex]),
    );
    return versions[selectedVersionIndex]?.commits ?? [];
  },
});

/**
 * Returns the appropriate Diff for the current pull request. By default, it
 * shows the Diff for the head commit of the PR compared to its parent, though
 * if the user has selected a pair of versions via the radio buttons, it returns
 * the Diff between those versions.
 */
export const gitHubPullRequestVersionDiff = selector<DiffWithCommitIDs | null>({
  key: 'gitHubPullRequestVersionDiff',
  get: ({get}) => {
    // For now, we special-case Sapling and ignore versions for the moment.
    const stackedPR = get(stackedPullRequest);
    if (stackedPR.type === 'sapling' && !isBottomOfSaplingStack(stackedPR.body)) {
      const fragments = get(stackedPullRequestFragments);
      if (fragments.length !== stackedPR.body.stack.length) {
        // This is unexpected: bail out.
        return null;
      }

      const {currentStackEntry: index} = stackedPR.body;
      const commitID = fragments[index].headRefOid;
      const baseCommitID = fragments[index + 1].headRefOid;
      return get(
        gitHubDiffForCommits({
          baseCommitID,
          commitID,
        }),
      );
    }

    const [client, comparableVersions] = get(
      waitForAll([gitHubClient, gitHubPullRequestComparableVersions]),
    );
    if (client == null) {
      return null;
    }

    const {beforeCommitID, afterCommitID} = comparableVersions;
    const afterBaseCommitID = get(gitHubPullRequestCommitBaseParent(afterCommitID))?.oid;
    if (beforeCommitID != null) {
      const beforeBaseCommitID = get(gitHubPullRequestCommitBaseParent(beforeCommitID))?.oid;
      if (beforeBaseCommitID != null && afterBaseCommitID != null) {
        // If the base parents are the same, then there was no rebase and the
        // two versions can be diffed directly
        if (beforeBaseCommitID === afterBaseCommitID) {
          return gitHubDiffForCommits({baseCommitID: beforeCommitID, commitID: afterCommitID});
        }

        const [beforeDiff, afterDiff] = get(
          waitForAll([
            gitHubDiffForCommits({baseCommitID: beforeBaseCommitID, commitID: beforeCommitID}),
            gitHubDiffForCommits({baseCommitID: afterBaseCommitID, commitID: afterCommitID}),
          ]),
        );
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
      return get(
        gitHubDiffForCommits({
          baseCommitID: afterBaseCommitID,
          commitID: afterCommitID,
        }),
      );
    }

    return null;
  },
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

export const gitHubCommit = selectorFamily<Commit | null, GitObjectID>({
  key: 'gitHubCommit',
  get:
    (oid: GitObjectID) =>
    ({get}) => {
      const client = get(gitHubClient);
      return client != null ? client.getCommit(oid) : null;
    },
});

export const gitHubCommitComparison = selectorFamily<
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

export const gitHubDiffCommitIDs = selector<DiffCommitIDs | null>({
  key: 'gitHubDiffCommitIDs',
  get: ({get}) => {
    const pullRequest = get(gitHubPullRequest);
    const diffWithCommitIDs =
      pullRequest != null ? get(gitHubPullRequestVersionDiff) : get(gitHubDiffForCurrentCommit);
    return diffWithCommitIDs?.commitIDs ?? null;
  },
});

export const gitHubDiffForCommitID = selectorFamily<DiffWithCommitIDs | null, GitObjectID>({
  key: 'gitHubDiffForCommitID',
  get:
    (oid: GitObjectID) =>
    ({get}) => {
      const [client, commit] = get(waitForAll([gitHubClient, gitHubCommit(oid)]));
      return client != null && commit != null ? diffCommitWithParent(commit, client) : null;
    },
});

export const gitHubDiffForCommits = selectorFamily<
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

export const gitHubDiffForCurrentCommit = selector<DiffWithCommitIDs | null>({
  key: 'gitHubDiffForCurrentCommit',
  get: ({get}) => {
    const client = get(gitHubClient);
    const commit = get(gitHubCurrentCommit);
    if (client != null && commit != null) {
      return diffCommitWithParent(commit, client);
    } else {
      return null;
    }
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

export const gitHubBlob = selectorFamily<Blob | null, string>({
  key: 'gitHubBlob',
  get:
    (oid: string) =>
    ({get}) => {
      const client = get(gitHubClient);
      return client != null ? client.getBlob(oid) : null;
    },
});

type FileMod = {
  before: GitObjectID | null;
  after: GitObjectID | null;
  path: string;
};

export type FileContentsDelta = {before: Blob | null; after: Blob | null};

export const nullAtom: RecoilValueReadOnly<null> = constSelector(null);

export const fileContentsDelta = selectorFamily<FileContentsDelta, FileMod>({
  key: 'fileContentsDelta',
  get:
    (mod: FileMod) =>
    ({get}) => {
      const [before, after] = get(
        waitForAll([
          mod.before != null ? gitHubBlob(mod.before) : nullAtom,
          mod.after != null ? gitHubBlob(mod.after) : nullAtom,
        ]),
      );
      return {before, after};
    },
});

export const gitHubPullRequests = selectorFamily<PullsWithPageInfo | null, PullsQueryInput>({
  key: 'gitHubPullRequests',
  get:
    input =>
    ({get}) => {
      const client = get(gitHubClient);
      if (client == null) {
        return null;
      }
      return client.getPullRequests(input);
    },
});

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

export const gitHubPullRequestPositionForLine = selectorFamily<
  number | null,
  {line: number; path: string; side: DiffSide}
>({
  key: 'gitHubPullRequestPositionForLine',
  get:
    ({line, path, side}) =>
    ({get}) => {
      const lineToPosition = get(gitHubPullRequestLineToPositionForFile(path));
      const lineToPositionForSide = lineToPosition?.[side];
      return lineToPositionForSide?.[line] ?? null;
    },
});

export const gitHubUserHomePageData = selector<UserHomePageQueryData | null>({
  key: 'gitHubUserHomePageData',
  get: ({get}) => {
    const token = get(gitHubTokenPersistence);
    if (token == null) {
      return null;
    }

    // Based on search query for https://github.com/pulls/review-requested
    const reviewRequestedQuery = 'is:open is:pr archived:false review-requested:@me';

    const graphQLEndpoint = get(gitHubGraphQLEndpoint);
    return queryGraphQL<UserHomePageQueryData, UserHomePageQueryVariables>(
      UserHomePageQuery,
      {reviewRequestedQuery},
      createRequestHeaders(token),
      graphQLEndpoint,
    );
  },
});

export const gitHubRepoAssignableUsersQuery = atom<string>({
  key: 'gitHubRepoAssignableUsersQuery',
  default: '',
});

export const gitHubRepoAssignableUsers = selector<UserFragment[]>({
  key: 'gitHubRepoAssignableUsers',
  get: async ({get}) => {
    const [client, query, username] = get(
      waitForAll([gitHubClient, gitHubRepoAssignableUsersQuery, gitHubUsername]),
    );
    if (client == null) {
      return [];
    }
    const users = await client.getRepoAssignableUsers(query);
    return users.filter(user => user.login !== username);
  },
});

export const gitHubRepoLabelsQuery = atom<string>({
  key: 'gitHubRepoLabelsQuery',
  default: '',
});

export const gitHubRepoLabels = selector<LabelFragment[]>({
  key: 'gitHubRepoLabels',
  get: ({get}) => {
    const [client, query] = get(waitForAll([gitHubClient, gitHubRepoLabelsQuery]));
    if (client == null) {
      return [];
    }
    return client.getRepoLabels(query);
  },
});

export const gitHubPullRequestLabels = atom<LabelFragment[]>({
  key: 'gitHubPullRequestLabels',
  default: [],
});

export type PullRequestReviewersList = {
  reviewers: ReadonlyArray<UserFragment>;
  reviewerIDs: ReadonlySet<string>;
};

export const gitHubPullRequestReviewers = atom<PullRequestReviewersList>({
  key: 'gitHubPullRequestReviewers',
  default: {
    reviewers: [],
    reviewerIDs: new Set(),
  },
});

type CheckRun = {
  workflowName: string | undefined;
} & CheckRunFragment;

export const gitHubPullRequestCheckRuns = selector<CheckRun[]>({
  key: 'gitHubPullRequestCheckRuns',
  get: ({get}) => {
    const pullRequest = get(gitHubPullRequest);
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
