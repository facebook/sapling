/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {VSCodeRepo, VSCodeReposList} from '../VSCodeRepo';
import type {CodeReviewProvider, DiffSummaries} from 'isl-server/src/CodeReviewProvider';
import type {RepositoryContext} from 'isl-server/src/serverTypes';
import type {CommitInfo} from 'isl/src/types';
import type * as vscode from 'vscode';

export class InlineCommentsProvider implements vscode.Disposable {
  private disposables: Array<vscode.Disposable> = [];
  private repoDisposables: Array<vscode.Disposable> = [];
  constructor(private reposList: VSCodeReposList, private ctx: RepositoryContext) {
    this.disposables.push(
      this.reposList.observeActiveRepos(repos => {
        for (const repo of repos) {
          this.setupFetchesForRepo(repo);
        }
      }),
    );
  }

  private setupFetchesForRepo(repo: VSCodeRepo): void {
    this.repoDisposables.forEach(d => d.dispose());
    if (repo) {
      const provider = repo.repo.codeReviewProvider;
      if (provider == null) {
        return;
      }

      this.repoDisposables.push(new InlineCommentsForRepo(repo, provider, this.ctx));
    }
  }

  dispose(): void {
    this.disposables.forEach(d => d.dispose());
  }
}

class InlineCommentsForRepo implements vscode.Disposable {
  private disposables: Array<vscode.Disposable> = [];
  constructor(
    private repo: VSCodeRepo,
    private provider: CodeReviewProvider,
    private ctx: RepositoryContext,
  ) {
    let currentHead = repo.repo.getHeadCommit();
    let mostRecentDiffInfos: DiffSummaries | undefined = undefined;

    // changing the head commit changes what diff ID we need to fetch comments for
    this.disposables.push(
      repo.repo.subscribeToHeadCommit(head => {
        currentHead = head;
        this.addCommentsForDiff(currentHead, mostRecentDiffInfos);
      }),
    );
    // periodically, diff summaries are refetched, in case comments are edited or added
    this.disposables.push(
      provider.onChangeDiffSummaries(result => {
        mostRecentDiffInfos = result.value ?? undefined;
        this.addCommentsForDiff(currentHead, mostRecentDiffInfos);
      }),
    );
  }

  private addCommentsForDiff(head: CommitInfo | undefined, summaries: DiffSummaries | undefined) {
    const diffId = head?.diffId;
    if (diffId == null) {
      return;
    }
    const headDiff = summaries?.get(diffId);
    if (headDiff == null) {
      return;
    }

    const numComments = headDiff.commentCount;
    if (numComments > 0) {
      this.provider.fetchComments?.(diffId).then(comments => {
        this.ctx.logger.info(`Updating ${comments.length} diff comments for diff ${diffId}`);
        // TODO: use fetched comments to add gutters to files
      });
    }
  }

  dispose(): void {
    this.disposables.forEach(d => d.dispose());
  }
}
