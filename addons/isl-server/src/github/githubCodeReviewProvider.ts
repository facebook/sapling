/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CodeReviewProvider} from '../CodeReviewProvider';
import type {Logger} from '../logger';
import type {
  CheckSuiteConnection,
  PullRequestState,
  YourPullRequestsQueryData,
  YourPullRequestsQueryVariables,
} from './generated/graphql';
import type {CodeReviewSystem, DiffSignalSummary, DiffId, Disposable, Result} from 'isl/src/types';

import {CheckStatusState, CheckConclusionState, YourPullRequestsQuery} from './generated/graphql';
import queryGraphQL from './queryGraphQL';
import {TypedEventEmitter} from 'shared/TypedEventEmitter';
import {debounce} from 'shared/debounce';

export type GitHubDiffSummary = {
  type: 'github';
  title: string;
  state: PullRequestState;
  number: DiffId;
  url: string;
  commentCount: number;
  anyUnresolvedComments: false;
  signalSummary?: DiffSignalSummary;
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
          numToFetch: 100,
        });
        if (allSummaries?.search.nodes == null) {
          this.diffSummaries.emit('data', new Map());
          return;
        }

        const map = new Map<DiffId, GitHubDiffSummary>();
        for (const summary of allSummaries.search.nodes) {
          if (summary != null && summary.__typename === 'PullRequest') {
            const id = String(summary.number);
            map.set(id, {
              type: 'github',
              title: summary.title,
              state: summary.state,
              number: id,
              url: summary.url,
              commentCount: summary.comments.totalCount,
              anyUnresolvedComments: false,
              signalSummary: githubCheckSuitesToCIStatus(
                summary.commits.nodes?.[0]?.commit.checkSuites as
                  | CheckSuiteConnection
                  | null
                  | undefined,
              ),
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
    return queryGraphQL<D, V>(query, variables);
  }

  public dispose() {
    this.diffSummaries.removeAllListeners();
  }
}

function githubCheckSuitesToCIStatus(
  checkSuites: CheckSuiteConnection | undefined | null,
): DiffSignalSummary {
  let anyInProgress = false;
  let anyWarning = false;
  for (const checkSuite of checkSuites?.nodes ?? []) {
    switch (checkSuite?.status) {
      case CheckStatusState.Completed:
        {
          switch (checkSuite?.conclusion) {
            case CheckConclusionState.Success:
              break;
            case CheckConclusionState.Neutral:
            case CheckConclusionState.Stale:
            case CheckConclusionState.Skipped:
              anyWarning = true;
              break;
            case CheckConclusionState.ActionRequired:
            case CheckConclusionState.StartupFailure:
            case CheckConclusionState.Cancelled:
            case CheckConclusionState.TimedOut:
            case CheckConclusionState.Failure:
              return 'failed'; // no need to look at other signals
          }
        }
        break;
      case CheckStatusState.Waiting:
      case CheckStatusState.Requested:
      case CheckStatusState.Queued:
      case CheckStatusState.Pending:
      case CheckStatusState.InProgress:
        anyInProgress = true;
        break;
    }
  }
  return anyWarning ? 'warning' : anyInProgress ? 'running' : 'pass';
}
