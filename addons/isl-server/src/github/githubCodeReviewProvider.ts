/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  CICheckRun,
  ClientToServerMessage,
  CodeReviewSystem,
  CommandArg,
  DiffComment,
  DiffId,
  DiffSignalSummary,
  Disposable,
  DraftPullRequestReviewThread,
  Hash,
  MergeableState,
  MergeStateStatus,
  Notification,
  OperationCommandProgressReporter,
  PullRequestReviewEvent,
  Result,
  ServerToClientMessage,
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
import {submitPullRequestReview} from './submitPullRequestReview';

export type GitHubDiffSummary = {
  type: 'github';
  title: string;
  commitMessage: string;
  state: PullRequestState | 'DRAFT' | 'MERGE_QUEUED';
  number: DiffId;
  /** GitHub GraphQL node ID, required for mutations like addPullRequestReview */
  nodeId: string;
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
  /** Mergeability state: MERGEABLE, CONFLICTING, or UNKNOWN */
  mergeable?: MergeableState;
  /** Detailed merge state status */
  mergeStateStatus?: MergeStateStatus;
  /** Individual CI check runs for detailed status display */
  ciChecks?: CICheckRun[];
  /** Whether viewer can bypass branch protection to merge */
  viewerCanMergeAsAdmin?: boolean;
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
      // Fetch all PRs (open, merged, closed) updated in the last 30 days
      // This allows "hide merged" filtering to work properly
      searchQuery: `repo:${this.codeReviewSystem.owner}/${this.codeReviewSystem.repo} is:pr updated:>=${dateFilter}`,
      // Reduced from 50 to avoid GitHub's 500k node limit (numToFetch × 100 commits × 100 contexts)
      numToFetch: 20,
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
              nodeId: summary.id,
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
              // Merge + CI status fields (Phase 12)
              mergeable: summary.mergeable as MergeableState | undefined,
              mergeStateStatus: summary.mergeStateStatus as MergeStateStatus | undefined,
              ciChecks: extractCIChecks(summary),
              viewerCanMergeAsAdmin: summary.viewerCanMergeAsAdmin ?? undefined,
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

    // Fetch thread IDs separately via reviewThreads query
    const threadMap = await this.fetchThreadInfo(diffId);

    return (
      [...comments, ...inline]?.filter(notEmpty).map(comment => {
        const reviewComment = comment as PullRequestReviewComment;
        // Match thread by path and line to get the threadId
        const threadKey = `${reviewComment.path ?? ''}:${reviewComment.line ?? ''}`;
        const threadInfo = threadMap.get(threadKey);
        return {
          author: comment.author?.login ?? '',
          authorAvatarUri: comment.author?.avatarUrl,
          html: comment.bodyHTML,
          created: new Date(comment.createdAt),
          filename: reviewComment.path ?? undefined,
          line: reviewComment.line ?? undefined,
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
          threadId: threadInfo?.id,
          isResolved: threadInfo?.isResolved,
        };
      }) ?? []
    );
  }

  /**
   * Fetch thread IDs for inline comments.
   * Returns a map of `path:line` -> { id, isResolved }
   */
  private async fetchThreadInfo(
    diffId: string,
  ): Promise<Map<string, {id: string; isResolved: boolean}>> {
    const threadQuery = `
      query PullRequestThreadsQuery($url: URI!) {
        resource(url: $url) {
          ... on PullRequest {
            reviewThreads(first: 100) {
              nodes {
                id
                isResolved
                path
                line
              }
            }
          }
        }
      }
    `;

    type ThreadQueryData = {
      resource?: {
        reviewThreads?: {
          nodes?: Array<{
            id: string;
            isResolved: boolean;
            path: string;
            line: number | null;
          } | null> | null;
        } | null;
      } | null;
    };

    try {
      const response = await this.query<ThreadQueryData, {url: string}>(threadQuery, {
        url: this.getPrUrl(diffId),
      });

      const threads = response?.resource?.reviewThreads?.nodes ?? [];
      const map = new Map<string, {id: string; isResolved: boolean}>();
      for (const thread of threads) {
        if (thread != null) {
          const key = `${thread.path}:${thread.line ?? ''}`;
          map.set(key, {id: thread.id, isResolved: thread.isResolved});
        }
      }
      return map;
    } catch (error) {
      this.logger.error('Failed to fetch thread info:', error);
      return new Map();
    }
  }

  /**
   * Resolve a comment thread.
   * Uses GitHub's resolveReviewThread GraphQL mutation.
   */
  public async resolveThread(threadId: string): Promise<void> {
    const mutation = `
      mutation ResolveThread($input: ResolveReviewThreadInput!) {
        resolveReviewThread(input: $input) {
          thread { id isResolved }
        }
      }
    `;

    const response = await this.query<
      {resolveReviewThread?: {thread?: {id: string; isResolved: boolean} | null} | null},
      {input: {threadId: string}}
    >(mutation, {input: {threadId}});

    if (response?.resolveReviewThread?.thread?.isResolved !== true) {
      throw new Error('Failed to resolve thread');
    }
  }

  /**
   * Unresolve a previously resolved comment thread.
   * Uses GitHub's unresolveReviewThread GraphQL mutation.
   */
  public async unresolveThread(threadId: string): Promise<void> {
    const mutation = `
      mutation UnresolveThread($input: UnresolveReviewThreadInput!) {
        unresolveReviewThread(input: $input) {
          thread { id isResolved }
        }
      }
    `;

    const response = await this.query<
      {unresolveReviewThread?: {thread?: {id: string; isResolved: boolean} | null} | null},
      {input: {threadId: string}}
    >(mutation, {input: {threadId}});

    if (response?.unresolveReviewThread?.thread?.isResolved !== false) {
      throw new Error('Failed to unresolve thread');
    }
  }

  /**
   * Reply to an existing comment thread.
   * Replies are submitted immediately (not batched) because they're on existing threads.
   */
  public async replyToThread(threadId: string, body: string): Promise<void> {
    const mutation = `
      mutation AddReply($threadId: ID!, $body: String!) {
        addPullRequestReviewThreadReply(input: {
          pullRequestReviewThreadId: $threadId,
          body: $body
        }) {
          comment {
            id
          }
        }
      }
    `;

    const response = await this.query<
      {addPullRequestReviewThreadReply?: {comment?: {id: string} | null} | null},
      {threadId: string; body: string}
    >(mutation, {threadId, body});

    if (response?.addPullRequestReviewThreadReply?.comment?.id == null) {
      throw new Error('Failed to add reply to thread');
    }
  }

  private query<D, V>(query: string, variables: V, timeoutMs?: number): Promise<D | undefined> {
    return queryGraphQL<D, V>(
      query,
      variables,
      this.codeReviewSystem.hostname,
      timeoutMs ?? DEFAULT_GH_FETCH_TIMEOUT,
    );
  }

  handleClientToServerMessage(
    message: ClientToServerMessage,
    postMessage: (message: ServerToClientMessage) => void,
  ): boolean {
    if (message.type === 'submitPullRequestReview') {
      this.handleSubmitPullRequestReview(message, postMessage);
      return true;
    }
    return false;
  }

  private async handleSubmitPullRequestReview(
    message: {
      type: 'submitPullRequestReview';
      pullRequestId: string;
      event: PullRequestReviewEvent;
      body?: string;
      threads?: DraftPullRequestReviewThread[];
    },
    postMessage: (message: ServerToClientMessage) => void,
  ): Promise<void> {
    try {
      const reviewId = await submitPullRequestReview(
        this.codeReviewSystem.hostname,
        message.pullRequestId,
        message.event,
        message.body,
        message.threads,
      );
      postMessage({
        type: 'submittedPullRequestReview',
        result: {value: {reviewId}},
      });
    } catch (error) {
      postMessage({
        type: 'submittedPullRequestReview',
        result: {error: error as Error},
      });
    }
  }

  public dispose() {
    this.diffSummaries.removeAllListeners();
    this.triggerDiffSummariesFetch.dispose();
  }

  /**
   * Run external commands via `gh` CLI.
   * Used for operations like `gh pr merge`, `gh pr close`, etc.
   */
  public async runExternalCommand(
    cwd: string,
    args: CommandArg[],
    onProgress: OperationCommandProgressReporter,
    _signal: AbortSignal,
  ): Promise<void> {
    const {ejeca} = await import('shared/ejeca');
    const {Internal} = await import('../Internal');

    // Convert CommandArg[] to string[] (filter out non-string args for gh commands)
    const stringArgs = args.filter((arg): arg is string => typeof arg === 'string');

    // Use gh CLI with the repo's hostname
    const ghPath = Internal.ghPath ?? 'gh';
    const hostname = this.codeReviewSystem.hostname;

    // Add hostname for GitHub Enterprise
    const ghArgs =
      hostname !== 'github.com'
        ? ['--hostname', hostname, ...stringArgs]
        : stringArgs;

    this.logger.info('running gh command:', ghPath, ghArgs.join(' '));
    onProgress('spawn');

    try {
      const result = await ejeca(ghPath, ghArgs, {
        cwd,
        env: {
          ...process.env,
          // Set GH_REPO to ensure gh knows which repo to operate on
          GH_REPO: `${this.codeReviewSystem.owner}/${this.codeReviewSystem.repo}`,
        },
      });

      if (result.stdout) {
        onProgress('stdout', result.stdout);
      }
      if (result.stderr) {
        onProgress('stderr', result.stderr);
      }
    } catch (error) {
      const err = error as Error & {stderr?: string; stdout?: string};
      if (err.stderr) {
        onProgress('stderr', err.stderr);
      }
      if (err.stdout) {
        onProgress('stdout', err.stdout);
      }
      throw error;
    }
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

/**
 * Extract CI check runs from the GraphQL PR data.
 * Handles both CheckRun (GitHub Checks API) and StatusContext (legacy status API).
 */
function extractCIChecks(pr: any): CICheckRun[] | undefined {
  const contexts = pr.commits?.nodes?.[0]?.commit?.statusCheckRollup?.contexts?.nodes;
  if (!contexts || contexts.length === 0) {
    return undefined;
  }

  return contexts
    .filter(notEmpty)
    .map(context => {
      if (context.__typename === 'CheckRun') {
        return {
          name: context.name ?? 'Unknown',
          status: (context.status ?? 'QUEUED') as CICheckRun['status'],
          conclusion: context.conclusion as CICheckRun['conclusion'],
          detailsUrl: context.detailsUrl,
        };
      } else {
        // StatusContext (legacy status API)
        return {
          name: context.context ?? 'Unknown',
          status: context.state === 'PENDING' ? 'PENDING' : 'COMPLETED',
          conclusion:
            context.state === 'SUCCESS'
              ? 'SUCCESS'
              : context.state === 'FAILURE'
                ? 'FAILURE'
                : context.state === 'ERROR'
                  ? 'FAILURE'
                  : undefined,
          detailsUrl: context.targetUrl,
        } as CICheckRun;
      }
    });
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
