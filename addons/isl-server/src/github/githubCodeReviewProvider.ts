/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CodeReviewProvider} from '../CodeReviewProvider';
import type {Logger} from '../logger';
import type {
  PullRequestReviewDecision,
  YourPullRequestsQueryData,
  YourPullRequestsQueryVariables,
} from './generated/graphql';
import type {CodeReviewSystem, DiffSignalSummary, DiffId, Disposable, Result} from 'isl/src/types';

import {PullRequestState, StatusState, YourPullRequestsQuery} from './generated/graphql';
import queryGraphQL from './queryGraphQL';
import {TypedEventEmitter} from 'shared/TypedEventEmitter';
import {debounce} from 'shared/debounce';

export type GitHubDiffSummary = {
  type: 'github';
  title: string;
  commitMessage: string;
  state: PullRequestState | 'DRAFT';
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

  public getDiffUrlMarkdown(diffId: DiffId): string {
    return `[#${diffId}](https://${this.codeReviewSystem.hostname}/${this.codeReviewSystem.owner}/${this.codeReviewSystem.repo}/pull/${diffId})`;
  }

  public getCommitHashUrlMarkdown(hash: string): string {
    return `[\`${hash.slice(0, 12)}\`](https://${this.codeReviewSystem.hostname}/${
      this.codeReviewSystem.owner
    }/${this.codeReviewSystem.repo}/commit/${hash})`;
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
