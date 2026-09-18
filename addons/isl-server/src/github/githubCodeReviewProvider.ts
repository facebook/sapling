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
  PullRequestReviewAction,
  PullRequestReviewData,
  Result,
} from 'isl/src/types';
import type {
  CodeReviewProvider,
  CreatedInlineComment,
  CreateInlineCommentInput,
} from '../CodeReviewProvider';
import type {Logger} from '../logger';
import type {
  MergeQueueSupportQueryData,
  MergeQueueSupportQueryVariables,
  PullRequestCommentsQueryData,
  PullRequestCommentsQueryVariables,
  PullRequestReviewDecision,
  YourPullRequestsQueryData,
  YourPullRequestsQueryVariables,
  YourPullRequestsWithoutMergeQueueQueryData,
  YourPullRequestsWithoutMergeQueueQueryVariables,
} from './generated/graphql';

import {TypedEventEmitter} from 'shared/TypedEventEmitter';
import {debounce} from 'shared/debounce';
import {ejeca} from 'shared/ejeca';
import {notEmpty} from 'shared/utils';
import {Internal} from '../Internal';
import {
  MergeQueueSupportQuery,
  PullRequestCommentsQuery,
  PullRequestState,
  StatusState,
  YourPullRequestsQuery,
  YourPullRequestsWithoutMergeQueueQuery,
} from './generated/graphql';
import {GitHubReviewService, type GitHubReviewRestRequest} from './githubReview';
import queryGraphQL from './queryGraphQL';
import queryREST from './queryREST';

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
  /**
   * Base of the Pull Request (public parent), as it is on GitHub (may be out of date).
   * Undefined if the ref no longer exists (e.g. branch deleted after merge).
   */
  base?: Hash;
  /**
   * Head of the Pull Request (topmost commit), as it is on GitHub (may be out of date).
   * Undefined if the ref no longer exists (e.g. branch deleted after merge).
   */
  head?: Hash;
  /** Name of the branch on GitHub, which should match the local bookmark */
  branchName?: string;
};

type GitHubCreatedReviewComment = {
  id?: number;
  html_url?: string;
  body?: string;
  created_at?: string;
  user?: {login?: string; avatar_url?: string};
};

function createdInlineComment(comment: GitHubCreatedReviewComment): CreatedInlineComment {
  return {
    id: comment.id == null ? undefined : String(comment.id),
    url: comment.html_url,
    body: comment.body ?? '',
    author: comment.user?.login ?? '',
    authorAvatarUri: comment.user?.avatar_url,
    created: comment.created_at == null ? new Date() : new Date(comment.created_at),
  };
}

const DEFAULT_GH_FETCH_TIMEOUT = 60_000; // 1 minute
const DIFF_SUMMARIES_AUTO_REFRESH_INTERVAL = 5 * 60_000;

type GitHubCodeReviewSystem = CodeReviewSystem & {type: 'github'};
export class GitHubCodeReviewProvider implements CodeReviewProvider {
  private reviewService: GitHubReviewService;

  constructor(
    private codeReviewSystem: GitHubCodeReviewSystem,
    private logger: Logger,
  ) {
    this.reviewService = new GitHubReviewService(
      {
        owner: codeReviewSystem.owner,
        repo: codeReviewSystem.repo,
        prUrl: diffId => this.getPrUrl(diffId),
      },
      {
        query: <D, V>(query: string, variables: V) => this.query<D, V>(query, variables),
        rest: request => this.runReviewRestRequest(request),
      },
    );
  }
  private diffSummaries = new TypedEventEmitter<'data', Map<DiffId, GitHubDiffSummary>>();
  private hasMergeQueueSupport: Promise<boolean> | null = null;
  private lastDiffSummariesFetchAt = 0;

  onChangeDiffSummaries(
    callback: (result: Result<Map<DiffId, GitHubDiffSummary>>) => unknown,
  ): Disposable {
    const handleData = (data: Map<DiffId, GitHubDiffSummary>) => callback({value: data});
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
    const variables = {
      // TODO: somehow base this query on the list of DiffIds
      // This is not very easy with github's graphql API, which doesn't allow more than 5 "OR"s in a search query.
      // But if we used one-query-per-diff we would reach rate limiting too quickly.
      searchQuery: `repo:${this.codeReviewSystem.owner}/${this.codeReviewSystem.repo} is:pr author:@me`,
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
    async (_diffs: Array<DiffId>, force = false) => {
      const now = Date.now();
      if (!force && now - this.lastDiffSummariesFetchAt < DIFF_SUMMARIES_AUTO_REFRESH_INTERVAL) {
        return;
      }
      // Record attempts as well as successful fetches. When GitHub rejects a request, retrying from
      // every repository poll only adds noise and prevents the rate limit from recovering cleanly.
      this.lastDiffSummariesFetchAt = now;

      try {
        const hasMergeQueueSupport = await this.detectMergeQueueSupport();
        this.logger.info('fetching github PR summaries');
        const allSummaries = await this.fetchYourPullRequestsGraphQL(hasMergeQueueSupport);
        if (allSummaries?.search.nodes == null) {
          this.diffSummaries.emit('data', new Map());
          return;
        }

        const map = new Map<DiffId, GitHubDiffSummary>();
        for (const summary of allSummaries.search.nodes) {
          if (summary != null && summary.__typename === 'PullRequest') {
            const id = String(summary.number);
            const commitMessage = summary.body.slice(summary.title.length + 1);
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
              base: summary.baseRef?.target?.oid,
              head: summary.headRef?.target?.oid,
              // Prefer headRefName: it persists even after the branch is deleted
              // (e.g. once the PR is merged), whereas headRef becomes null.
              branchName: summary.headRefName ?? summary.headRef?.name,
            });
          }
        }
        this.logger.info(`fetched ${map.size} github PR summaries`);
        this.diffSummaries.emit('data', map);
      } catch (error) {
        this.logger.info('error fetching github PR summaries: ', error);
        this.diffSummaries.emit('error', error as Error);
      }
    },
    2000,
    undefined,
    /* leading */ true,
  );

  public async fetchComments(
    diffId: string,
    options?: {includeReactions?: boolean},
  ): Promise<DiffComment[]> {
    const response = await this.query<
      PullRequestCommentsQueryData,
      PullRequestCommentsQueryVariables
    >(PullRequestCommentsQuery, {
      url: this.getPrUrl(diffId),
      numToFetch: 50,
      includeReactions: options?.includeReactions ?? true,
    });

    if (response == null) {
      throw new Error(`Failed to fetch comments for ${diffId}`);
    }

    const pr = response?.resource as
      (PullRequestCommentsQueryData['resource'] & {__typename: 'PullRequest'}) | undefined;

    const comments = pr?.comments.nodes ?? [];

    const inline = pr?.reviewThreads.nodes?.filter(notEmpty) ?? [];

    this.logger.info(`fetched ${comments?.length} comments for github PR ${diffId}}`);

    return [
      ...comments.filter(notEmpty).map(comment => {
        return {
          id: comment.id,
          author: comment.author?.login ?? '',
          authorAvatarUri: comment.author?.avatarUrl,
          content: comment.body,
          html: comment.bodyHTML,
          created: new Date(comment.createdAt),
          reactions:
            comment.reactions?.nodes?.flatMap(reaction =>
              reaction?.user?.login == null
                ? []
                : [{name: reaction.user.login, reaction: reaction.content}],
            ) ?? [],
          replies: [], // PR top level doesn't have nested replies, you just reply to their name
        };
      }),
      ...inline
        .map(thread => {
          const threadComments = thread.comments.nodes?.filter(notEmpty) ?? [];
          const first = threadComments[0];
          if (first == null) {
            return null;
          }
          const mapComment = (comment: (typeof threadComments)[number]): DiffComment => ({
            id: String(comment.databaseId ?? comment.id),
            url:
              comment.databaseId == null
                ? undefined
                : `${this.getPrUrl(diffId)}#discussion_r${comment.databaseId}`,
            author: comment.author?.login ?? '',
            authorAvatarUri: comment.author?.avatarUrl,
            content: comment.body,
            html: comment.bodyHTML,
            created: new Date(comment.createdAt),
            filename: thread.path,
            line: thread.line ?? thread.originalLine ?? undefined,
            startLine: thread.startLine ?? thread.originalStartLine ?? undefined,
            side: thread.diffSide,
            reactions:
              comment.reactions?.nodes?.flatMap(reaction =>
                reaction?.user?.login == null
                  ? []
                  : [{name: reaction.user.login, reaction: reaction.content}],
              ) ?? [],
            replies: [],
            isResolved: thread.isResolved,
          });
          const result = mapComment(first);
          result.replies = threadComments.slice(1).map(mapComment);
          return result;
        })
        .filter(notEmpty),
    ];
  }

  public async createInlineComment(
    diffId: string,
    input: CreateInlineCommentInput,
  ): Promise<CreatedInlineComment> {
    const endpoint = `repos/${this.codeReviewSystem.owner}/${this.codeReviewSystem.repo}/pulls/${diffId}/comments`;
    if (input.replyTo != null) {
      const comment = await queryREST<GitHubCreatedReviewComment>(
        endpoint,
        this.codeReviewSystem.hostname,
        'POST',
        {
          body: input.body,
          in_reply_to: Number(input.replyTo),
        },
      );
      return createdInlineComment(comment);
    }

    const pullRequest = await queryREST<{head: {sha: string}}>(
      `repos/${this.codeReviewSystem.owner}/${this.codeReviewSystem.repo}/pulls/${diffId}`,
      this.codeReviewSystem.hostname,
    );
    const comment = await queryREST<GitHubCreatedReviewComment>(
      endpoint,
      this.codeReviewSystem.hostname,
      'POST',
      {
        body: input.body,
        commit_id: pullRequest.head.sha,
        path: input.path,
        line: input.line,
        side: input.side,
        ...(input.startLine == null || input.startLine === input.line
          ? {}
          : {start_line: input.startLine, start_side: input.side}),
      },
    );
    return createdInlineComment(comment);
  }

  public fetchPullRequestReview(diffId: string): Promise<PullRequestReviewData> {
    return this.reviewService.fetch(diffId);
  }

  public runPullRequestReviewAction(
    diffId: string,
    action: PullRequestReviewAction,
  ): Promise<PullRequestReviewData> {
    return this.reviewService.runAction(diffId, action);
  }

  private async runReviewRestRequest(request: GitHubReviewRestRequest): Promise<void> {
    const args = [
      'api',
      '--hostname',
      this.codeReviewSystem.hostname,
      '--method',
      request.method,
      request.endpoint,
    ];
    for (const [key, value] of Object.entries(request.fields ?? {})) {
      args.push(typeof value === 'number' ? '-F' : '-f', `${key}=${value}`);
    }
    await ejeca('gh', args, {
      env: {
        ...((await Internal.additionalGhEnvVars?.()) ?? {}),
      },
    });
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
