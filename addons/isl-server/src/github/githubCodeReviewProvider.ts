/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CodeReviewProvider} from '../CodeReviewProvider';
import type {Logger} from '../logger';
import type {
  PullRequestCommentsQueryData,
  PullRequestCommentsQueryVariables,
  PullRequestReviewComment,
  PullRequestReviewDecision,
  ReactionContent,
  YourPullRequestsQueryData,
  YourPullRequestsQueryVariables,
} from './generated/graphql';
import type {
  CodeReviewSystem,
  DiffSignalSummary,
  DiffId,
  Disposable,
  Result,
  DiffComment,
} from 'isl/src/types';
import type {ParsedDiff} from 'shared/patch/parse';

import {
  PullRequestCommentsQuery,
  PullRequestState,
  StatusState,
  YourPullRequestsQuery,
} from './generated/graphql';
import queryGraphQL from './queryGraphQL';
import {TypedEventEmitter} from 'shared/TypedEventEmitter';
import {debounce} from 'shared/debounce';
import {parsePatch} from 'shared/patch/parse';
import {notEmpty} from 'shared/utils';

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
};

type GitHubCodeReviewSystem = CodeReviewSystem & {type: 'github'};
export class GitHubCodeReviewProvider implements CodeReviewProvider {
  constructor(private codeReviewSystem: GitHubCodeReviewSystem, private logger: Logger) {}
  private diffSummaries = new TypedEventEmitter<'data', Map<DiffId, GitHubDiffSummary>>();

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

  triggerDiffSummariesFetch = debounce(
    async () => {
      try {
        this.logger.info('fetching github PR summaries');
        const allSummaries = await this.query<
          YourPullRequestsQueryData,
          YourPullRequestsQueryVariables
        >(YourPullRequestsQuery, {
          // TODO: somehow base this query on the list of DiffIds
          // This is not very easy with github's graphql API, which doesn't allow more than 5 "OR"s in a search query.
          // But if we used one-query-per-diff we would reach rate limiting too quickly.
          searchQuery: `repo:${this.codeReviewSystem.owner}/${this.codeReviewSystem.repo} is:pr author:@me`,
          numToFetch: 50,
        });
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

  private query<D, V>(query: string, variables: V): Promise<D | undefined> {
    return queryGraphQL<D, V>(query, variables, this.codeReviewSystem.hostname);
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
