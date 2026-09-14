/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  PullRequestReviewAction,
  PullRequestReviewComment,
  PullRequestReviewData,
  PullRequestReviewEvent,
} from 'isl/src/types';
import type {
  AddPullRequestReviewThreadData,
  AddPullRequestReviewThreadVariables,
  CreatePendingPullRequestReviewData,
  CreatePendingPullRequestReviewVariables,
  CreateSubmittedPullRequestReviewData,
  CreateSubmittedPullRequestReviewVariables,
  DiffSide,
  PullRequestReviewEvent as GitHubPullRequestReviewEvent,
  PullRequestReviewQueryData,
  PullRequestReviewQueryVariables,
  ResolvePullRequestReviewThreadData,
  ResolvePullRequestReviewThreadVariables,
  SubmitPullRequestReviewData,
  SubmitPullRequestReviewVariables,
  UnresolvePullRequestReviewThreadData,
  UnresolvePullRequestReviewThreadVariables,
} from './generated/graphql';

import {notEmpty} from 'shared/utils';
import {
  AddPullRequestReviewThread,
  CreatePendingPullRequestReview,
  CreateSubmittedPullRequestReview,
  PullRequestReviewQuery,
  PullRequestReviewState,
  ResolvePullRequestReviewThread,
  SubmitPullRequestReview,
  UnresolvePullRequestReviewThread,
} from './generated/graphql';

export type GitHubReviewRestRequest = {
  method: 'POST' | 'PATCH' | 'DELETE';
  endpoint: string;
  fields?: Record<string, string | number>;
};

export type GitHubReviewDeps = {
  query<D, V>(query: string, variables: V): Promise<D | undefined>;
  rest(request: GitHubReviewRestRequest): Promise<void>;
};

export type GitHubReviewRepo = {
  owner: string;
  repo: string;
  prUrl(diffId: string): string;
};

/** Read and mutate GitHub pull request review threads. */
export class GitHubReviewService {
  constructor(
    private repository: GitHubReviewRepo,
    private deps: GitHubReviewDeps,
  ) {}

  async fetch(diffId: string): Promise<PullRequestReviewData> {
    const response = await this.deps.query<
      PullRequestReviewQueryData,
      PullRequestReviewQueryVariables
    >(PullRequestReviewQuery, {
      url: this.repository.prUrl(diffId),
      numToFetch: 100,
    });

    if (response == null || response.resource?.__typename !== 'PullRequest') {
      throw new Error(`Failed to fetch review threads for pull request #${diffId}.`);
    }

    const viewer = response.viewer.login;
    const pendingReview = response.resource.reviews?.nodes
      ?.filter(notEmpty)
      .find(
        review =>
          review.state === PullRequestReviewState.Pending && review.author?.login === viewer,
      );

    return {
      pullRequestId: response.resource.id,
      headOid: response.resource.headRefOid,
      pendingReviewId: pendingReview?.id,
      pendingReviewCommitOid: pendingReview?.commit?.oid,
      threads:
        response.resource.reviewThreads.nodes?.filter(notEmpty).map(thread => ({
          id: thread.id,
          path: thread.path,
          line: thread.line ?? undefined,
          originalLine: thread.originalLine ?? undefined,
          startLine: thread.startLine ?? undefined,
          originalStartLine: thread.originalStartLine ?? undefined,
          side: thread.diffSide,
          startSide: thread.startDiffSide ?? undefined,
          isOutdated: thread.isOutdated,
          isResolved: thread.isResolved,
          viewerCanReply: thread.viewerCanReply,
          viewerCanResolve: thread.viewerCanResolve,
          viewerCanUnresolve: thread.viewerCanUnresolve,
          comments: thread.comments.nodes?.filter(notEmpty).map(mapComment) ?? [],
        })) ?? [],
    };
  }

  async runAction(diffId: string, action: PullRequestReviewAction): Promise<PullRequestReviewData> {
    switch (action.type) {
      case 'createComment':
        await this.createComment(diffId, action);
        break;
      case 'reply':
        await this.deps.rest({
          method: 'POST',
          endpoint: this.reviewReplyEndpoint(diffId, action.commentDatabaseId),
          fields: {body: requireBody(action.body)},
        });
        break;
      case 'editComment':
        await this.deps.rest({
          method: 'PATCH',
          endpoint: this.reviewCommentEndpoint(action.commentDatabaseId),
          fields: {body: requireBody(action.body)},
        });
        break;
      case 'deleteComment':
        await this.deps.rest({
          method: 'DELETE',
          endpoint: this.reviewCommentEndpoint(action.commentDatabaseId),
        });
        break;
      case 'setResolved':
        await this.setResolved(action.threadId, action.resolved);
        break;
      case 'submitReview':
        await this.submitReview(diffId, action.event, action.body, action.expectedHeadOid);
        break;
    }

    return this.fetch(diffId);
  }

  private async createComment(
    diffId: string,
    action: Extract<PullRequestReviewAction, {type: 'createComment'}>,
  ): Promise<void> {
    const review = await this.fetch(diffId);
    assertExpectedHead(review, action.expectedHeadOid, diffId);
    const body = requireBody(action.body);
    if (action.mode === 'single') {
      await this.deps.rest({
        method: 'POST',
        endpoint: this.reviewCommentsEndpoint(diffId),
        fields: {
          body,
          commit_id: action.commitOid,
          path: action.path,
          line: action.line,
          side: action.side,
          ...(action.startLine == null
            ? {}
            : {
                start_line: action.startLine,
                start_side: action.startSide ?? action.side,
              }),
        },
      });
      return;
    }

    if (review.pendingReviewId != null && review.pendingReviewCommitOid !== action.commitOid) {
      throw new Error(
        'The pending GitHub review belongs to a different commit. Submit it before starting ' +
          'a pending review on this commit, or add this as a single comment.',
      );
    }
    const pendingReviewId =
      review.pendingReviewId ?? (await this.createPendingReview(review, action.commitOid));
    const response = await this.deps.query<
      AddPullRequestReviewThreadData,
      AddPullRequestReviewThreadVariables
    >(AddPullRequestReviewThread, {
      pullRequestReviewId: pendingReviewId,
      body,
      path: action.path,
      line: action.line,
      side: action.side as DiffSide,
      ...(action.startLine == null
        ? {}
        : {
            startLine: action.startLine,
            startSide: (action.startSide ?? action.side) as DiffSide,
          }),
    });
    if (response?.addPullRequestReviewThread?.thread?.id == null) {
      throw new Error(`GitHub did not add the review comment to pull request #${diffId}.`);
    }
  }

  private async createPendingReview(
    review: PullRequestReviewData,
    commitOid: string,
  ): Promise<string> {
    const response = await this.deps.query<
      CreatePendingPullRequestReviewData,
      CreatePendingPullRequestReviewVariables
    >(CreatePendingPullRequestReview, {pullRequestId: review.pullRequestId, commitOid});
    const id = response?.addPullRequestReview?.pullRequestReview?.id;
    if (id == null) {
      throw new Error('GitHub did not create a pending pull request review.');
    }
    return id;
  }

  private async submitReview(
    diffId: string,
    event: PullRequestReviewEvent,
    body: string,
    expectedHeadOid: string,
  ): Promise<void> {
    const review = await this.fetch(diffId);
    assertExpectedHead(review, expectedHeadOid, diffId);
    const githubEvent = event as GitHubPullRequestReviewEvent;
    if (review.pendingReviewId == null) {
      const response = await this.deps.query<
        CreateSubmittedPullRequestReviewData,
        CreateSubmittedPullRequestReviewVariables
      >(CreateSubmittedPullRequestReview, {
        pullRequestId: review.pullRequestId,
        event: githubEvent,
        body,
      });
      if (response?.addPullRequestReview?.pullRequestReview?.id == null) {
        throw new Error(`GitHub did not submit the review for pull request #${diffId}.`);
      }
      return;
    }

    const response = await this.deps.query<
      SubmitPullRequestReviewData,
      SubmitPullRequestReviewVariables
    >(SubmitPullRequestReview, {
      pullRequestReviewId: review.pendingReviewId,
      event: githubEvent,
      body,
    });
    if (response?.submitPullRequestReview?.pullRequestReview?.id == null) {
      throw new Error(`GitHub did not submit the pending review for pull request #${diffId}.`);
    }
  }

  private async setResolved(threadId: string, resolved: boolean): Promise<void> {
    if (resolved) {
      const response = await this.deps.query<
        ResolvePullRequestReviewThreadData,
        ResolvePullRequestReviewThreadVariables
      >(ResolvePullRequestReviewThread, {threadId});
      if (response?.resolveReviewThread?.thread?.isResolved !== true) {
        throw new Error('GitHub did not resolve the review thread.');
      }
    } else {
      const response = await this.deps.query<
        UnresolvePullRequestReviewThreadData,
        UnresolvePullRequestReviewThreadVariables
      >(UnresolvePullRequestReviewThread, {threadId});
      if (response?.unresolveReviewThread?.thread?.isResolved !== false) {
        throw new Error('GitHub did not reopen the review thread.');
      }
    }
  }

  private reviewCommentsEndpoint(diffId: string): string {
    return `repos/${this.repository.owner}/${this.repository.repo}/pulls/${diffId}/comments`;
  }

  private reviewReplyEndpoint(diffId: string, commentId: number): string {
    return `${this.reviewCommentsEndpoint(diffId)}/${commentId}/replies`;
  }

  private reviewCommentEndpoint(commentId: number): string {
    return `repos/${this.repository.owner}/${this.repository.repo}/pulls/comments/${commentId}`;
  }
}

function mapComment(
  comment: NonNullable<
    NonNullable<
      Extract<
        NonNullable<PullRequestReviewQueryData['resource']>,
        {__typename: 'PullRequest'}
      >['reviewThreads']['nodes']
    >[number]
  >['comments']['nodes'] extends Array<infer T> | null | undefined
    ? NonNullable<T>
    : never,
): PullRequestReviewComment {
  return {
    id: comment.id,
    databaseId: comment.databaseId ?? undefined,
    author: comment.author?.login ?? '',
    authorAvatarUri: comment.author?.avatarUrl,
    body: comment.body,
    html: comment.bodyHTML,
    created: new Date(comment.publishedAt ?? comment.createdAt),
    url: comment.url,
    state: comment.state,
    viewerCanDelete: comment.viewerCanDelete,
    viewerCanUpdate: comment.viewerCanUpdate,
    reactions:
      comment.reactions.nodes
        ?.filter(
          (reaction): reaction is NonNullable<typeof reaction> & {user: {login: string}} =>
            reaction?.user?.login != null,
        )
        .map(reaction => ({name: reaction.user.login, reaction: reaction.content})) ?? [],
  };
}

function requireBody(body: string): string {
  const trimmed = body.trim();
  if (trimmed === '') {
    throw new Error('Review comments cannot be empty.');
  }
  return trimmed;
}

function assertExpectedHead(
  review: PullRequestReviewData,
  expectedHeadOid: string,
  diffId: string,
): void {
  if (review.headOid !== expectedHeadOid) {
    throw new Error(
      `Pull request #${diffId} changed while it was open. Download the latest version before reviewing it.`,
    );
  }
}
