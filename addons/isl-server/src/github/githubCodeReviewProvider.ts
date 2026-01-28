/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  CodeReviewSystem,
  DiffComment,
  DiffId,
  DiffSignalSummary,
  Disposable,
  Hash,
  Notification,
  Result,
} from 'isl/src/types';
import type {CodeReviewProvider} from '../CodeReviewProvider';
import type {Logger} from '../logger';
import type {
  MergeQueueSupportQueryData,
  MergeQueueSupportQueryVariables,
  PullRequestCommentsQueryData,
  PullRequestCommentsQueryVariables,
  PullRequestReviewComment,
  PullRequestReviewDecision,
  ReactionContent,
  YourPullRequestsQueryData,
  YourPullRequestsQueryVariables,
  YourPullRequestsWithoutMergeQueueQueryData,
  YourPullRequestsWithoutMergeQueueQueryVariables,
} from './generated/graphql';

import {TypedEventEmitter} from 'shared/TypedEventEmitter';
import {debounce} from 'shared/debounce';
import {notEmpty} from 'shared/utils';
import {
  MergeQueueSupportQuery,
  PullRequestCommentsQuery,
  PullRequestState,
  StatusState,
  YourPullRequestsQuery,
  YourPullRequestsWithoutMergeQueueQuery,
} from './generated/graphql';
import {parseStackInfo, type StackEntry} from './parseStackInfo';
import queryGraphQL from './queryGraphQL';

export type GitHubDiffSummary = {
  type: 'github';
  title: string;
  commitMessage: string;
  state: PullRequestState | 'DRAFT' | 'MERGE_QUEUED';
  number: DiffId;
  url: string;
  commentCount: number;
  anyUnresolvedComments: false;
  signalSummary?: DiffSignalSummary;
  reviewDecision?: PullRequestReviewDecision;
  /** Base of the Pull Request (public parent), as it is on GitHub (may be out of date) */
  base: Hash;
  /** Head of the Pull Request (topmost commit), as it is on GitHub (may be out of date) */
  head: Hash;
  /** Name of the branch on GitHub, which should match the local bookmark */
  branchName?: string;
  /** Stack info parsed from PR body Sapling footer. Top-to-bottom order (first = top of stack). */
  stackInfo?: StackEntry[];
  /** Author login (GitHub username) */
  author?: string;
  /** Author avatar URL */
  authorAvatarUrl?: string;
};

const DEFAULT_GH_FETCH_TIMEOUT = 60_000; // 1 minute

export type DiffSummariesData = {
  summaries: Map<DiffId, GitHubDiffSummary>;
  currentUser?: string;
};

type GitHubCodeReviewSystem = CodeReviewSystem & {type: 'github'};
export class GitHubCodeReviewProvider implements CodeReviewProvider {
  constructor(
    private codeReviewSystem: GitHubCodeReviewSystem,
    private logger: Logger,
  ) {}
  private diffSummaries = new TypedEventEmitter<'data', DiffSummariesData>();
  private hasMergeQueueSupport: Promise<boolean> | null = null;

  onChangeDiffSummaries(
    callback: (result: Result<Map<DiffId, GitHubDiffSummary>>, currentUser?: string) => unknown,
  ): Disposable {
    const handleData = (data: DiffSummariesData) =>
      callback({value: data.summaries}, data.currentUser);
    const handleError = (error: Error) => callback({error});
    this.diffSummaries.on('data', handleData);
    this.diffSummaries.on('error', handleError);
    return {
      dispose: () => {
        this.diffSummaries.off('data', handleData);
        this.diffSummaries.off('error', handleError);
      },
    };
  }

  private detectMergeQueueSupport(): Promise<boolean> {
    if (this.hasMergeQueueSupport == null) {
      this.hasMergeQueueSupport = (async (): Promise<boolean> => {
        this.logger.info('detecting if merge queue is supported');
        const data = await this.query<MergeQueueSupportQueryData, MergeQueueSupportQueryVariables>(
          MergeQueueSupportQuery,
          {},
          10_000,
        ).catch(err => {
          this.logger.info('failed to detect merge queue support', err);
          return undefined;
        });
        const hasMergeQueueSupport = data?.__type != null;
        this.logger.info('set merge queue support to ' + hasMergeQueueSupport);
        return hasMergeQueueSupport;
      })();
    }
    return this.hasMergeQueueSupport;
  }

  private fetchYourPullRequestsGraphQL(
    includeMergeQueue: boolean,
  ): Promise<YourPullRequestsQueryData | undefined> {
    // Calculate date 30 days ago for the updated filter
    const thirtyDaysAgo = new Date();
    thirtyDaysAgo.setDate(thirtyDaysAgo.getDate() - 30);
    const dateFilter = thirtyDaysAgo.toISOString().split('T')[0];

    const variables = {
      // Fetch all open PRs in the repo updated in the last 30 days
      searchQuery: `repo:${this.codeReviewSystem.owner}/${this.codeReviewSystem.repo} is:pr is:open updated:>=${dateFilter}`,
      numToFetch: 50,
    };
    if (includeMergeQueue) {
      return this.query<YourPullRequestsQueryData, YourPullRequestsQueryVariables>(
        YourPullRequestsQuery,
        variables,
      );
    } else {
      return this.query<
        YourPullRequestsWithoutMergeQueueQueryData,
        YourPullRequestsWithoutMergeQueueQueryVariables
      >(YourPullRequestsWithoutMergeQueueQuery, variables);
    }
  }

  triggerDiffSummariesFetch = debounce(
    async () => {
      try {
        const hasMergeQueueSupport = await this.detectMergeQueueSupport();
        this.logger.info('fetching github PR summaries');
        const allSummaries = await this.fetchYourPullRequestsGraphQL(hasMergeQueueSupport);
        if (allSummaries?.search.nodes == null) {
          this.diffSummaries.emit('data', {summaries: new Map()});
          return;
        }
        const currentUser = allSummaries.viewer?.login;

        const map = new Map<DiffId, GitHubDiffSummary>();
        for (const summary of allSummaries.search.nodes) {
          if (summary != null && summary.__typename === 'PullRequest') {
            const id = String(summary.number);
            const commitMessage = summary.body.slice(summary.title.length + 1);
            if (summary.baseRef?.target == null || summary.headRef?.target == null) {
              this.logger.warn(`PR #${id} is missing base or head ref, skipping.`);
              continue;
            }
            // Parse stack info from the PR body (Sapling footer format)
            const stackInfo = parseStackInfo(summary.body) ?? undefined;

            map.set(id, {
              type: 'github',
              title: summary.title,
              commitMessage,
              // For some reason, `isDraft` is a separate boolean and not a state,
              // but we generally treat it as its own state in the UI.
              state:
                summary.isDraft && summary.state === PullRequestState.Open
                  ? 'DRAFT'
                  : summary.mergeQueueEntry != null
                    ? 'MERGE_QUEUED'
                    : summary.state,
              number: id,
              url: summary.url,
              commentCount: summary.comments.totalCount,
              anyUnresolvedComments: false,
              signalSummary: githubStatusRollupStateToCIStatus(
                summary.commits.nodes?.[0]?.commit.statusCheckRollup?.state,
              ),
              reviewDecision: summary.reviewDecision ?? undefined,
              base: summary.baseRef.target.oid,
              head: summary.headRef.target.oid,
              branchName: summary.headRef.name,
              stackInfo,
              author: summary.author?.login ?? undefined,
              authorAvatarUrl: summary.author?.avatarUrl ?? undefined,
            });
          }
        }
        this.logger.info(`fetched ${map.size} github PR summaries`);
        this.diffSummaries.emit('data', {summaries: map, currentUser});
      } catch (error) {
        this.logger.info('error fetching github PR summaries: ', error);
        this.diffSummaries.emit('error', error as Error);
      }
    },
    2000,
    undefined,
    /* leading */ true,
  );

  public async fetchComments(diffId: string): Promise<DiffComment[]> {
    const response = await this.query<
      PullRequestCommentsQueryData,
      PullRequestCommentsQueryVariables
    >(PullRequestCommentsQuery, {
      url: this.getPrUrl(diffId),
      numToFetch: 50,
    });

    if (response == null) {
      throw new Error(`Failed to fetch comments for ${diffId}`);
    }

    const pr = response?.resource as
      | (PullRequestCommentsQueryData['resource'] & {__typename: 'PullRequest'})
      | undefined;

    const comments = pr?.comments.nodes ?? [];

    const inline =
      pr?.reviews?.nodes?.filter(notEmpty).flatMap(review => review.comments.nodes) ?? [];

    this.logger.info(`fetched ${comments?.length} comments for github PR ${diffId}}`);

    return (
      [...comments, ...inline]?.filter(notEmpty).map(comment => {
        return {
          author: comment.author?.login ?? '',
          authorAvatarUri: comment.author?.avatarUrl,
          html: comment.bodyHTML,
          created: new Date(comment.createdAt),
          filename: (comment as PullRequestReviewComment).path ?? undefined,
          line: (comment as PullRequestReviewComment).line ?? undefined,
          reactions:
            comment.reactions?.nodes
              ?.filter(
                (reaction): reaction is {user: {login: string}; content: ReactionContent} =>
                  reaction?.user?.login != null,
              )
              .map(reaction => ({
                name: reaction.user.login,
                reaction: reaction.content,
              })) ?? [],
          replies: [], // PR top level doesn't have nested replies, you just reply to their name
        };
      }) ?? []
    );
  }

  private query<D, V>(query: string, variables: V, timeoutMs?: number): Promise<D | undefined> {
    return queryGraphQL<D, V>(
      query,
      variables,
      this.codeReviewSystem.hostname,
      timeoutMs ?? DEFAULT_GH_FETCH_TIMEOUT,
    );
  }

  public dispose() {
    this.diffSummaries.removeAllListeners();
    this.triggerDiffSummariesFetch.dispose();
  }

  public getSummaryName(): string {
    return `github:${this.codeReviewSystem.hostname}/${this.codeReviewSystem.owner}/${this.codeReviewSystem.repo}`;
  }

  public getPrUrl(diffId: DiffId): string {
    return `https://${this.codeReviewSystem.hostname}/${this.codeReviewSystem.owner}/${this.codeReviewSystem.repo}/pull/${diffId}`;
  }

  public getDiffUrlMarkdown(diffId: DiffId): string {
    return `[#${diffId}](${this.getPrUrl(diffId)})`;
  }

  public getCommitHashUrlMarkdown(hash: string): string {
    return `[\`${hash.slice(0, 12)}\`](https://${this.codeReviewSystem.hostname}/${
      this.codeReviewSystem.owner
    }/${this.codeReviewSystem.repo}/commit/${hash})`;
  }

  getRemoteFileURL(
    path: string,
    publicCommitHash: string | null,
    selectionStart?: {line: number; char: number},
    selectionEnd?: {line: number; char: number},
  ): string {
    const {hostname, owner, repo} = this.codeReviewSystem;
    let url = `https://${hostname}/${owner}/${repo}/blob/${publicCommitHash ?? 'HEAD'}/${path}`;
    if (selectionStart != null) {
      url += `#L${selectionStart.line + 1}`;
      if (
        selectionEnd &&
        (selectionEnd.line !== selectionStart.line || selectionEnd.char !== selectionStart.char)
      ) {
        url += `C${selectionStart.char + 1}-L${selectionEnd.line + 1}C${selectionEnd.char + 1}`;
      }
    }
    return url;
  }

  public async fetchNotifications(): Promise<Notification[]> {
    const {hostname, owner, repo} = this.codeReviewSystem;
    const {ejeca} = await import('shared/ejeca');
    const {Internal} = await import('../Internal');

    try {
      // Fetch notifications from GitHub REST API filtered by this repo
      const {stdout} = await ejeca(
        'gh',
        [
          'api',
          `/repos/${owner}/${repo}/notifications`,
          '--hostname',
          hostname,
          '-H',
          'Accept: application/vnd.github+json',
        ],
        {
          env: {
            ...((await Internal.additionalGhEnvVars?.()) ?? {}),
          },
        },
      );

      const rawNotifications: GitHubNotificationResponse[] = JSON.parse(stdout);
      const notifications: Notification[] = [];

      for (const notif of rawNotifications) {
        // Only process PR notifications
        if (notif.subject.type !== 'PullRequest' || !notif.subject.url) {
          continue;
        }

        // Extract PR number from the API URL
        const prMatch = notif.subject.url.match(/\/pulls\/(\d+)$/);
        if (!prMatch) {
          continue;
        }
        const prNumber = parseInt(prMatch[1], 10);

        // Map notification reason to our notification type
        let notificationType: Notification['type'];
        let reviewState: Notification['reviewState'];

        switch (notif.reason) {
          case 'review_requested':
            notificationType = 'review-request';
            break;
          case 'mention':
          case 'team_mention':
            notificationType = 'mention';
            break;
          case 'approval':
            notificationType = 'review-received';
            reviewState = 'APPROVED';
            break;
          case 'changes_requested':
            notificationType = 'review-received';
            reviewState = 'CHANGES_REQUESTED';
            break;
          case 'comment':
            notificationType = 'review-received';
            reviewState = 'COMMENTED';
            break;
          default:
            // Skip unsupported notification types
            continue;
        }

        // Try to get actor info from the PR details
        let actor = 'Unknown';
        let actorAvatarUrl: string | undefined;

        try {
          const prDetailsResult = await ejeca(
            'gh',
            [
              'api',
              `/repos/${owner}/${repo}/pulls/${prNumber}`,
              '--hostname',
              hostname,
              '-H',
              'Accept: application/vnd.github+json',
            ],
            {
              env: {
                ...((await Internal.additionalGhEnvVars?.()) ?? {}),
              },
            },
          );
          const prDetails: GitHubPRDetails = JSON.parse(prDetailsResult.stdout);
          if (prDetails.user) {
            actor = prDetails.user.login;
            actorAvatarUrl = prDetails.user.avatar_url;
          }
        } catch {
          // Continue without actor info
        }

        notifications.push({
          id: `${notificationType}-${prNumber}-${notif.id}`,
          type: notificationType,
          prNumber,
          prTitle: notif.subject.title,
          prUrl: `https://${hostname}/${owner}/${repo}/pull/${prNumber}`,
          repoName: notif.repository.full_name,
          actor,
          actorAvatarUrl,
          timestamp: new Date(notif.updated_at),
          reviewState,
        });
      }

      this.logger.info(`fetched ${notifications.length} github notifications`);
      return notifications;
    } catch (error) {
      this.logger.error('error fetching github notifications:', error);
      throw error;
    }
  }
}

function githubStatusRollupStateToCIStatus(state: StatusState | undefined): DiffSignalSummary {
  switch (state) {
    case undefined:
    case StatusState.Expected:
      return 'no-signal';
    case StatusState.Pending:
      return 'running';
    case StatusState.Error:
    case StatusState.Failure:
      return 'failed';
    case StatusState.Success:
      return 'pass';
  }
}

type GitHubNotificationResponse = {
  id: string;
  reason: string;
  subject: {
    title: string;
    url: string;
    type: string;
  };
  repository: {
    full_name: string;
  };
  updated_at: string;
};

type GitHubPRDetails = {
  number: number;
  html_url: string;
  user?: {
    login: string;
    avatar_url?: string;
  };
};

type GitHubReviewDetails = {
  user?: {
    login: string;
    avatar_url?: string;
  };
  state: string;
};
