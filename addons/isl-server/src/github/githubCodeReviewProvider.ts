/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
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
  PullRequestCommentsQueryData,
  PullRequestCommentsQueryVariables,
  PullRequestReviewComment,
  PullRequestReviewDecision,
  ReactionContent,
} from './generated/graphql';

import {TypedEventEmitter} from 'shared/TypedEventEmitter';
import {debounce} from 'shared/debounce';
import {notEmpty} from 'shared/utils';
import {
  PullRequestCommentsQuery,
  PullRequestState,
  StatusState,
} from './generated/graphql';
import type {CombinedPRQueryData, CombinedPRQueryVariables} from './CombinedPRQuery';
import {CombinedPRQuery} from './CombinedPRQuery';
import {parseStackInfo, type StackEntry} from './parseStackInfo';
import queryGraphQL from './queryGraphQL';
import {publishPullRequest} from './publishPullRequest';
import {submitPullRequestReview} from './submitPullRequestReview';

export type GitHubDiffSummary = {
  type: 'github';
  title: string;
  commitMessage: string;
  state: PullRequestState | 'DRAFT';
  number: DiffId;
  /** GitHub GraphQL node ID, required for mutations like addPullRequestReview */
  nodeId: string;
  url: string;
  commentCount: number;
  anyUnresolvedComments: false;
  signalSummary?: DiffSignalSummary;
  reviewDecision?: PullRequestReviewDecision;
  /** Latest review state per reviewer (from latestReviews query) */
  latestReviews?: Array<{state: string; author?: string; publishedAt?: string}>;
  /** Base of the Pull Request (public parent), as it is on GitHub (may be out of date) */
  base: Hash;
  /** Head of the Pull Request (topmost commit), as it is on GitHub (may be out of date) */
  head: Hash;
  /** Name of the branch on GitHub, which should match the local bookmark */
  branchName?: string;
  /** Name of the base branch this PR targets (e.g., "main" or "pr4565" for stacked PRs) */
  baseRefName?: string;
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
  /** Time range in days for filtering PRs. undefined means "all time". */
  private timeRangeDays: number | undefined = 7;

  setTimeRange(days: number | undefined): void {
    this.timeRangeDays = days;
  }

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

  private async fetchAllPRData(): Promise<CombinedPRQueryData | undefined> {
    const repoFilter = `repo:${this.codeReviewSystem.owner}/${this.codeReviewSystem.repo}`;
    const openQuery = `${repoFilter} is:pr is:open sort:updated-desc`;
    let closedQuery = `${repoFilter} is:pr -is:open sort:updated-desc`;
    if (this.timeRangeDays != null) {
      const dateAgo = new Date();
      dateAgo.setDate(dateAgo.getDate() - this.timeRangeDays);
      const dateFilter = dateAgo.toISOString().split('T')[0];
      closedQuery += ` updated:>=${dateFilter}`;
    }

    const variables: CombinedPRQueryVariables = {openQuery, closedQuery, numToFetch: 100};
    return this.query<CombinedPRQueryData, CombinedPRQueryVariables>(CombinedPRQuery, variables);
  }

  triggerDiffSummariesFetch = debounce(
    async () => {
      try {
        this.logger.info('fetching github PR summaries');
        const result = await this.fetchAllPRData();

        if (result?.rateLimit != null) {
          const {remaining, cost} = result.rateLimit;
          if (remaining < 500) {
            this.logger.warn(
              `GitHub API rate limit low: ${remaining} remaining (cost: ${cost})`,
            );
          }
        }

        const openNodes = result?.open?.nodes ?? [];
        const closedNodes = result?.closed?.nodes ?? [];
        if (openNodes.length === 0 && closedNodes.length === 0) {
          this.diffSummaries.emit('data', {summaries: new Map()});
          return;
        }
        const currentUser = result?.viewer?.login;
        const seen = new Set<number>();
        const map = new Map<DiffId, GitHubDiffSummary>();

        for (const summary of [...openNodes, ...closedNodes]) {
          if (summary != null && summary.__typename === 'PullRequest') {
            if (seen.has(summary.number)) {
              continue;
            }
            seen.add(summary.number);
            const id = String(summary.number);
            const commitMessage = summary.body.slice(summary.title.length + 1);
            const hasMissingRefs =
              summary.baseRef?.target == null || summary.headRef?.target == null;
            if (hasMissingRefs && summary.state === PullRequestState.Open) {
              this.logger.warn(`PR #${id} is open but missing base or head ref, skipping.`);
              continue;
            }
            const stackInfo = parseStackInfo(summary.body) ?? undefined;

            map.set(id, {
              type: 'github',
              title: summary.title,
              commitMessage,
              state:
                summary.isDraft && summary.state === PullRequestState.Open
                  ? 'DRAFT'
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
              latestReviews: summary.latestReviews?.nodes
                ?.filter((r): r is NonNullable<typeof r> => r != null)
                .map(r => ({state: r.state, author: r.author?.login, publishedAt: r.publishedAt ?? undefined})),
              base: summary.baseRef?.target?.oid ?? '',
              head: summary.headRef?.target?.oid ?? '',
              branchName: summary.headRef?.name ?? '',
              baseRefName: summary.baseRef?.name ?? undefined,
              stackInfo,
              author: summary.author?.login ?? undefined,
              authorAvatarUrl: summary.author?.avatarUrl ?? undefined,
              // mergeable, mergeStateStatus, and viewerCanMergeAsAdmin are omitted
              // from the bulk query to avoid GitHub 502 timeouts. They are only
              // needed in merge/review mode and can be lazy-loaded per-PR.
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
          id: (comment as {id?: string}).id,
          author: comment.author?.login ?? '',
          authorAvatarUri: comment.author?.avatarUrl,
          html: comment.bodyHTML,
          content: (comment as {body?: string}).body,
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

  /**
   * Post an immediate issue comment on a PR (not part of a review).
   * Uses GitHub's addComment mutation with the PR's node ID.
   */
  public async addIssueComment(subjectId: string, body: string): Promise<void> {
    const mutation = `
      mutation AddComment($subjectId: ID!, $body: String!) {
        addComment(input: {subjectId: $subjectId, body: $body}) {
          commentEdge {
            node { id }
          }
        }
      }
    `;

    const response = await this.query<
      {addComment?: {commentEdge?: {node?: {id: string} | null} | null} | null},
      {subjectId: string; body: string}
    >(mutation, {subjectId, body});

    if (response?.addComment?.commentEdge?.node?.id == null) {
      throw new Error('Failed to add comment');
    }
  }

  /**
   * Edit an existing comment (issue comment or review comment).
   * Detects type from the node ID prefix: IC_ = issue comment, else review comment.
   */
  public async editComment(commentId: string, body: string): Promise<void> {
    const isIssueComment = commentId.startsWith('IC_');

    if (isIssueComment) {
      const mutation = `
        mutation UpdateIssueComment($id: ID!, $body: String!) {
          updateIssueComment(input: {id: $id, body: $body}) {
            issueComment { id }
          }
        }
      `;
      const response = await this.query<
        {updateIssueComment?: {issueComment?: {id: string} | null} | null},
        {id: string; body: string}
      >(mutation, {id: commentId, body});
      if (response?.updateIssueComment?.issueComment?.id == null) {
        throw new Error('Failed to edit issue comment');
      }
    } else {
      const mutation = `
        mutation UpdateReviewComment($id: ID!, $body: String!) {
          updatePullRequestReviewComment(input: {pullRequestReviewCommentId: $id, body: $body}) {
            pullRequestReviewComment { id }
          }
        }
      `;
      const response = await this.query<
        {updatePullRequestReviewComment?: {pullRequestReviewComment?: {id: string} | null} | null},
        {id: string; body: string}
      >(mutation, {id: commentId, body});
      if (response?.updatePullRequestReviewComment?.pullRequestReviewComment?.id == null) {
        throw new Error('Failed to edit review comment');
      }
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
    if (message.type === 'publishPullRequest') {
      this.handlePublishPullRequest(message, postMessage);
      return true;
    }
    if (message.type === 'fetchPRMergeState') {
      this.handleFetchPRMergeState(message, postMessage);
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

  private async handlePublishPullRequest(
    message: {
      type: 'publishPullRequest';
      pullRequestId: string;
    },
    postMessage: (message: ServerToClientMessage) => void,
  ): Promise<void> {
    try {
      const pullRequestId = await publishPullRequest(
        this.codeReviewSystem.hostname,
        message.pullRequestId,
      );
      postMessage({
        type: 'publishedPullRequest',
        result: {value: {pullRequestId}},
      });
    } catch (error) {
      postMessage({
        type: 'publishedPullRequest',
        result: {error: error as Error},
      });
    }
  }

  private async handleFetchPRMergeState(
    message: {type: 'fetchPRMergeState'; prNumber: string},
    postMessage: (message: ServerToClientMessage) => void,
  ): Promise<void> {
    const mergeStateQuery = `
      query PRMergeState($url: URI!) {
        resource(url: $url) {
          ... on PullRequest {
            mergeable
            mergeStateStatus
            viewerCanMergeAsAdmin
          }
        }
      }
    `;
    type MergeStateData = {
      resource?: {
        mergeable?: string;
        mergeStateStatus?: string;
        viewerCanMergeAsAdmin?: boolean;
      } | null;
    };

    try {
      const response = await this.query<MergeStateData, {url: string}>(mergeStateQuery, {
        url: this.getPrUrl(message.prNumber),
      });
      postMessage({
        type: 'fetchedPRMergeState',
        prNumber: message.prNumber,
        result: {
          value: {
            mergeable: response?.resource?.mergeable as any,
            mergeStateStatus: response?.resource?.mergeStateStatus as any,
            viewerCanMergeAsAdmin: response?.resource?.viewerCanMergeAsAdmin,
          },
        },
      });
    } catch (error) {
      postMessage({
        type: 'fetchedPRMergeState',
        prNumber: message.prNumber,
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
      onProgress('exit', result.exitCode ?? 0);
    } catch (error) {
      const err = error as Error & {stderr?: string; stdout?: string; exitCode?: number};
      if (err.stderr) {
        onProgress('stderr', err.stderr);
      }
      if (err.stdout) {
        onProgress('stdout', err.stdout);
      }
      onProgress('exit', err.exitCode ?? 1);
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
