/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {VSCodeRepo, VSCodeReposList} from '../VSCodeRepo';
import type {CodeReviewProvider, DiffSummaries} from 'isl-server/src/CodeReviewProvider';
import type {RepositoryContext} from 'isl-server/src/serverTypes';
import type {CommitInfo, DiffComment} from 'isl/src/types';

import * as vscode from 'vscode';

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

declare module 'vscode' {
  export interface WebviewEditorInset {
    readonly editor: TextEditor;
    readonly line: number;
    readonly height: number;
    readonly webview: Webview;
    readonly onDidDispose: Event<void>;
    dispose(): void;
  }

  // eslint-disable-next-line @typescript-eslint/no-namespace
  export namespace window {
    export function createWebviewTextEditorInset(
      editor: TextEditor,
      line: number,
      height: number,
      options?: WebviewOptions,
    ): WebviewEditorInset;
  }
}

class InlineCommentsForRepo implements vscode.Disposable {
  private disposables: Array<vscode.Disposable> = [];
  private currentCommentsPerFile: Map<string, Array<DiffComment>> = new Map();
  private currentDecorations: Array<vscode.Disposable> = [];
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

    this.disposables.push(
      vscode.window.onDidChangeActiveTextEditor(() => {
        this.updateActiveFileDecorations();
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
        for (const comment of comments) {
          if (comment.filename) {
            const existing = this.currentCommentsPerFile.get(comment.filename) ?? [];
            existing.push(comment);
            this.currentCommentsPerFile.set(comment.filename, existing);
          }
        }
        this.ctx.logger.info(
          `Found comments for files: `,
          [...this.currentCommentsPerFile.values()].join(', '),
        );
        this.updateActiveFileDecorations();
      });
    }
  }

  private disposeActiveFileDecorations() {
    this.currentDecorations.forEach(d => d.dispose());
    this.currentDecorations = [];
  }
  private updateActiveFileDecorations() {
    this.disposeActiveFileDecorations();
    const editor = vscode.window.activeTextEditor;
    if (editor?.viewColumn == null || editor.document.uri.scheme !== 'file') {
      // this is not a real editor
      return;
    }

    const filepath = editor.document.uri.fsPath;
    const repoRelative = this.repo.repoRelativeFsPath(editor.document.uri);
    this.ctx.logger.info('udpate decorations for', filepath, repoRelative);
    if (repoRelative == null) {
      return;
    }

    const comments = this.currentCommentsPerFile.get(repoRelative);
    if (comments == null) {
      return;
    }
    for (const comment of comments) {
      if (!comment.line) {
        continue;
      }
      const range = new vscode.Range(comment.line, 0, comment.line, 0);

      const HEIGHT_IN_LINES = 1;
      const inset = vscode.window.createWebviewTextEditorInset(
        editor,
        range.start.line - 1,
        HEIGHT_IN_LINES,
        {
          enableScripts: true,
        },
      );

      inset.webview.html = `<html><body style="padding: 0;">${comment.html}</body></html>`;
      this.currentDecorations.push(inset);

      const decoration = vscode.window.createTextEditorDecorationType({
        overviewRulerLane: vscode.OverviewRulerLane.Left,
        overviewRulerColor: '#D6D8E8',
        rangeBehavior: vscode.DecorationRangeBehavior.ClosedClosed,
      });
      editor.setDecorations(decoration, [{range}]);
      this.currentDecorations.push(decoration);
    }
  }

  dispose(): void {
    this.disposeActiveFileDecorations();
    this.disposables.forEach(d => d.dispose());
  }
}
