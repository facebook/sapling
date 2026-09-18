/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  CreatedInlineComment,
  CreateInlineCommentInput,
} from 'isl-server/src/CodeReviewProvider';
import type {Repository} from 'isl-server/src/Repository';
import type {RepositoryContext} from 'isl-server/src/serverTypes';
import type {DiffComment} from 'isl/src/types';
import type {Comparison} from 'shared/Comparison';

import {repositoryCache} from 'isl-server/src/RepositoryCache';
import * as path from 'node:path';
import {ComparisonType} from 'shared/Comparison';
import * as vscode from 'vscode';
import {decodeSaplingDiffUri, SAPLING_DIFF_PROVIDER_SCHEME} from './DiffContentProvider';

const COMMENT_CONTROLLER_ID = 'sapling-review-comments';
const REFRESH_INTERVAL_MS = 5 * 60_000;
const COMMENT_CACHE_TTL_MS = 60_000;

type ReviewContext = {
  uri: vscode.Uri;
  path: string;
  repo: Repository;
  diffId?: string;
  activeRange?: vscode.Range;
  remoteThreads: Set<vscode.CommentThread>;
  refreshGeneration: number;
};

type CommentFetchEntry = {
  promise: Promise<DiffComment[]>;
  settledAt: number | null;
};

/**
 * Shares one comments request between all open files for a pull request. VS Code may ask for
 * commenting ranges many times while laying out an editor, so the cache also prevents those UI
 * callbacks from turning into GitHub requests.
 */
export class ReviewCommentFetchCache {
  private readonly entries = new Map<string, CommentFetchEntry>();

  constructor(
    private readonly ttlMs = COMMENT_CACHE_TTL_MS,
    private readonly now: () => number = Date.now,
  ) {}

  get(key: string, fetchComments: () => Promise<DiffComment[]>): Promise<DiffComment[]> {
    const existing = this.entries.get(key);
    if (
      existing != null &&
      (existing.settledAt == null || this.now() - existing.settledAt < this.ttlMs)
    ) {
      return existing.promise;
    }

    const entry: CommentFetchEntry = {
      promise: Promise.resolve().then(fetchComments),
      settledAt: null,
    };
    this.entries.set(key, entry);
    entry.promise.then(
      () => {
        if (this.entries.get(key) === entry) {
          entry.settledAt = this.now();
        }
      },
      () => {
        if (this.entries.get(key) === entry) {
          this.entries.delete(key);
        }
      },
    );
    return entry.promise;
  }

  invalidate(key: string): void {
    this.entries.delete(key);
  }

  clear(): void {
    this.entries.clear();
  }
}

class ReviewComment implements vscode.Comment {
  constructor(
    public body: string | vscode.MarkdownString,
    public mode: vscode.CommentMode,
    public author: vscode.CommentAuthorInformation,
    public parent: vscode.CommentThread,
    public remoteId?: string,
    public contextValue?: string,
    public timestamp?: Date,
    public label?: string,
    public replyTo?: string,
  ) {}
}

class GitHubReviewCommentsProvider implements vscode.Disposable {
  private readonly controller = vscode.comments.createCommentController(
    COMMENT_CONTROLLER_ID,
    'Sapling Review Comments',
  );
  private readonly contexts = new Map<string, ReviewContext>();
  private readonly draftThreads = new Set<vscode.CommentThread>();
  private readonly commentFetchCache = new ReviewCommentFetchCache();
  private readonly disposables: Array<vscode.Disposable> = [];
  private readonly refreshTimer: ReturnType<typeof setInterval>;
  private readonly activeRangeDecoration = vscode.window.createTextEditorDecorationType({
    isWholeLine: true,
    backgroundColor: new vscode.ThemeColor('editorCommentsWidget.rangeActiveBackground'),
    borderColor: new vscode.ThemeColor('editorGutter.commentRangeForeground'),
    borderStyle: 'solid',
    borderWidth: '0 0 0 3px',
  });

  constructor(private readonly ctx: RepositoryContext) {
    this.controller.options = {
      prompt: 'Add a GitHub review comment',
      placeHolder: 'Write a comment…',
    };
    this.controller.commentingRangeProvider = {
      provideCommentingRanges: document => {
        const isReviewDocument = this.isReviewDocument(document.uri);
        if (isReviewDocument) {
          this.trackEncodedUri(document.uri);
        }
        if (!isReviewDocument || document.lineCount === 0) {
          return [];
        }
        return [new vscode.Range(0, 0, document.lineCount - 1, 0)];
      },
    };

    this.disposables.push(
      vscode.commands.registerCommand(
        'sapling.submit-review-comment',
        (reply: vscode.CommentReply) => this.submitReply(reply),
      ),
      vscode.commands.registerCommand(
        'sapling.suggest-review-change',
        (reply: vscode.CommentReply) => this.startSuggestion(reply),
      ),
      vscode.commands.registerCommand(
        'sapling.submit-review-suggestion',
        (comment: ReviewComment) => this.submitSuggestion(comment),
      ),
      vscode.commands.registerCommand(
        'sapling.cancel-review-suggestion',
        (comment: ReviewComment) => this.discardComment(comment),
      ),
      vscode.commands.registerCommand('sapling.retry-review-comment', (comment: ReviewComment) =>
        this.retryComment(comment),
      ),
      vscode.commands.registerCommand('sapling.discard-review-comment', (comment: ReviewComment) =>
        this.discardComment(comment),
      ),
      vscode.commands.registerCommand(
        'sapling.cancel-empty-review-comment',
        (reply: vscode.CommentReply) => {
          this.clearActiveRange(reply.thread);
          reply.thread.dispose();
        },
      ),
      vscode.window.onDidChangeActiveTextEditor(editor => {
        if (editor != null) {
          this.trackEncodedUri(editor.document.uri);
          void this.refreshByUri(editor.document.uri);
        }
        this.updateActiveRangeDecorations();
      }),
      vscode.window.onDidChangeTextEditorSelection(event => this.handleSelectionChange(event)),
      vscode.workspace.onDidCloseTextDocument(document => this.untrack(document.uri)),
    );
    this.disposables.push({
      dispose: repositoryCache.onChangeActiveRepos(() => {
        for (const document of vscode.workspace.textDocuments) {
          this.trackEncodedUri(document.uri);
        }
      }),
    });
    for (const document of vscode.workspace.textDocuments) {
      this.trackEncodedUri(document.uri);
    }
    this.refreshTimer = setInterval(() => this.refreshVisible(), REFRESH_INTERVAL_MS);
  }

  track(uri: vscode.Uri, fileUri: vscode.Uri, comparison: Comparison): void {
    const hash = reviewCommitHash(comparison);
    if (hash != null) {
      this.trackCommit(uri, fileUri, hash);
    }
  }

  private isReviewDocument(uri: vscode.Uri): boolean {
    if (uri.scheme !== SAPLING_DIFF_PROVIDER_SCHEME) {
      return this.contexts.has(uri.toString());
    }
    try {
      return decodeSaplingDiffUri(uri).reviewCommitHash != null;
    } catch {
      return false;
    }
  }

  private trackEncodedUri(uri: vscode.Uri): void {
    if (uri.scheme !== SAPLING_DIFF_PROVIDER_SCHEME) {
      return;
    }
    try {
      const {originalUri, reviewCommitHash} = decodeSaplingDiffUri(uri);
      if (reviewCommitHash != null) {
        this.trackCommit(uri, originalUri, reviewCommitHash);
      }
    } catch {
      // Another provider owns malformed URIs; it will report the content error.
    }
  }

  private trackCommit(uri: vscode.Uri, fileUri: vscode.Uri, hash: string): void {
    const repo = repositoryCache.cachedRepositoryForPath(fileUri.fsPath);
    if (repo == null || repo.info.codeReviewSystem.type !== 'github') {
      return;
    }

    const relativePath = path
      .relative(repo.info.repoRoot, fileUri.fsPath)
      .split(path.sep)
      .join('/');
    const key = uri.toString();
    const existing = this.contexts.get(key);
    if (existing != null) {
      return;
    }

    const context: ReviewContext = {
      uri,
      path: relativePath,
      repo,
      remoteThreads: new Set(),
      refreshGeneration: 0,
    };
    this.contexts.set(key, context);
    this.ctx.logger.info(`Enabled GitHub review comments for ${relativePath} at ${hash}`);
    void repo
      .lookupCommits(repo.initialConnectionContext, [hash])
      .then(commits => {
        const commit = commits.values().next().value;
        if (commit?.diffId == null) {
          this.ctx.logger.info(`Commit ${hash} is not linked to a GitHub pull request`);
          return;
        }
        context.diffId = commit.diffId;
        if (this.isVisible(context.uri)) {
          return this.refresh(context);
        }
      })
      .catch(error => this.ctx.logger.error('Failed to load review comments', error));
  }

  private async refreshByUri(uri: vscode.Uri): Promise<void> {
    const context = this.contexts.get(uri.toString());
    if (context != null) {
      await this.refresh(context);
    }
  }

  private refreshVisible(): void {
    for (const context of this.contexts.values()) {
      if (this.isVisible(context.uri)) {
        void this.refresh(context);
      }
    }
  }

  private isVisible(uri: vscode.Uri): boolean {
    const key = uri.toString();
    return vscode.window.visibleTextEditors.some(editor => editor.document.uri.toString() === key);
  }

  private commentFetchKey(context: ReviewContext): string | null {
    return context.diffId == null ? null : `${context.repo.info.repoRoot}\0${context.diffId}`;
  }

  private untrack(uri: vscode.Uri): void {
    const key = uri.toString();
    const context = this.contexts.get(key);
    if (context == null) {
      return;
    }
    for (const thread of context.remoteThreads) {
      thread.dispose();
    }
    this.contexts.delete(key);

    const fetchKey = this.commentFetchKey(context);
    if (
      fetchKey != null &&
      ![...this.contexts.values()].some(other => this.commentFetchKey(other) === fetchKey)
    ) {
      this.commentFetchCache.invalidate(fetchKey);
    }
  }

  private async refresh(context: ReviewContext): Promise<void> {
    const provider = context.repo.codeReviewProvider;
    if (
      context.diffId == null ||
      provider?.fetchComments == null ||
      [...this.draftThreads].some(thread => thread.uri.toString() === context.uri.toString())
    ) {
      return;
    }
    const diffId = context.diffId;
    const fetchComments = provider.fetchComments.bind(provider);
    const generation = ++context.refreshGeneration;
    try {
      const fetchKey = this.commentFetchKey(context);
      if (fetchKey == null) {
        return;
      }
      const comments = await this.commentFetchCache.get(fetchKey, () =>
        fetchComments(diffId, {includeReactions: false}),
      );
      if (generation !== context.refreshGeneration) {
        return;
      }
      for (const thread of context.remoteThreads) {
        thread.dispose();
      }
      context.remoteThreads.clear();
      for (const comment of comments) {
        if (comment.filename !== context.path || comment.line == null || comment.side === 'LEFT') {
          continue;
        }
        const startLine = Math.max(0, (comment.startLine ?? comment.line) - 1);
        const endLine = Math.max(startLine, comment.line - 1);
        const thread = this.controller.createCommentThread(
          context.uri,
          new vscode.Range(startLine, 0, endLine, 0),
          [],
        );
        const allComments = [comment, ...comment.replies];
        thread.comments = allComments.map(item => this.toVSCodeComment(item, thread, context));
        thread.canReply = comment.isResolved !== true;
        thread.label = comment.isResolved === true ? 'Resolved' : undefined;
        thread.collapsibleState = vscode.CommentThreadCollapsibleState.Collapsed;
        context.remoteThreads.add(thread);
      }
    } catch (error) {
      this.ctx.logger.error('Failed to refresh GitHub review comments', error);
    }
  }

  private toVSCodeComment(
    comment: DiffComment,
    thread: vscode.CommentThread,
    context: ReviewContext,
  ): ReviewComment {
    return new ReviewComment(
      reviewCommentMarkdown(
        comment.content ?? comment.html,
        comment.url,
        this.reviewStackUrl(context),
      ),
      vscode.CommentMode.Preview,
      {
        name: comment.authorName ?? comment.author,
        iconPath:
          comment.authorAvatarUri == null ? undefined : vscode.Uri.parse(comment.authorAvatarUri),
      },
      thread,
      comment.id,
      undefined,
      comment.created,
    );
  }

  private async submitReply(reply: vscode.CommentReply): Promise<void> {
    const body = reply.text.trim();
    if (body === '') {
      return;
    }
    const first = reply.thread.comments[0] as ReviewComment | undefined;
    await this.submit(reply.thread, body, first?.remoteId);
  }

  private startSuggestion(reply: vscode.CommentReply): void {
    const document = vscode.workspace.textDocuments.find(
      document => document.uri.toString() === reply.thread.uri.toString(),
    );
    if (document == null) {
      vscode.window.showErrorMessage('Could not read the selected diff lines.');
      return;
    }
    const selected = selectedLineText(document, reply.thread.range);
    const body = suggestionBody(selected, reply.text);
    const comment = new ReviewComment(
      body,
      vscode.CommentMode.Editing,
      {name: 'You'},
      reply.thread,
      undefined,
      'saplingSuggestionDraft',
    );
    reply.thread.comments = [comment];
    this.setActiveRange(reply.thread);
    this.draftThreads.add(reply.thread);
    reply.thread.contextValue = 'saplingSuggestionDraft';
    reply.thread.canReply = false;
  }

  private async submitSuggestion(comment: ReviewComment): Promise<void> {
    const thread = comment.parent;
    thread.comments = thread.comments.filter(item => item !== comment);
    this.draftThreads.delete(thread);
    thread.contextValue = undefined;
    await this.submit(thread, commentBody(comment));
  }

  private async retryComment(comment: ReviewComment): Promise<void> {
    const thread = comment.parent;
    thread.comments = thread.comments.filter(item => item !== comment);
    this.draftThreads.delete(thread);
    thread.contextValue = undefined;
    await this.submit(thread, commentBody(comment), comment.replyTo);
  }

  private discardComment(comment: ReviewComment): void {
    const thread = comment.parent;
    this.draftThreads.delete(thread);
    const remaining = thread.comments.filter(item => item !== comment);
    if (remaining.length === 0) {
      this.clearActiveRange(thread);
      thread.dispose();
      return;
    }
    thread.comments = remaining;
    thread.contextValue = undefined;
    thread.canReply = true;
    this.contexts.get(thread.uri.toString())?.remoteThreads.add(thread);
  }

  private async submit(
    thread: vscode.CommentThread,
    body: string,
    replyTo?: string,
  ): Promise<void> {
    const context = this.contexts.get(thread.uri.toString());
    const provider = context?.repo.codeReviewProvider;
    context?.remoteThreads.delete(thread);
    this.draftThreads.add(thread);
    this.setActiveRange(thread);
    const pending = new ReviewComment(
      body,
      vscode.CommentMode.Preview,
      {name: 'You'},
      thread,
      undefined,
      'saplingPostingComment',
      new Date(),
      'Posting…',
      replyTo,
    );
    thread.comments = [...thread.comments, pending];
    thread.contextValue = 'saplingPostingComment';
    thread.canReply = false;
    thread.collapsibleState = vscode.CommentThreadCollapsibleState.Expanded;

    if (context?.diffId == null || provider?.createInlineComment == null) {
      this.markCommentAsFailed(pending);
      vscode.window.showErrorMessage('This diff is not linked to a GitHub pull request.');
      return;
    }

    const input: CreateInlineCommentInput = {
      body,
      path: context.path,
      line: thread.range.end.line + 1,
      startLine: thread.range.start.line + 1,
      side: 'RIGHT',
      replyTo,
    };
    const createInlineComment = provider.createInlineComment.bind(provider);
    const diffId = context.diffId;
    try {
      this.ctx.logger.info(`Posting GitHub review comment to PR ${diffId} on ${context.path}`);
      const created = await vscode.window.withProgress(
        {location: vscode.ProgressLocation.Notification, title: 'Posting GitHub review comment…'},
        () => createInlineComment(diffId, input),
      );
      this.markCommentAsPosted(pending, created, body, context);
      const fetchKey = this.commentFetchKey(context);
      if (fetchKey != null) {
        this.commentFetchCache.invalidate(fetchKey);
      }
      this.draftThreads.delete(thread);
      thread.contextValue = undefined;
      thread.canReply = true;
      context.remoteThreads.add(thread);
      provider.triggerDiffSummariesFetch([diffId], true, true);
      this.ctx.logger.info(`Posted GitHub review comment ${created?.url ?? created?.id ?? ''}`);
    } catch (error) {
      this.markCommentAsFailed(pending);
      this.ctx.logger.error('Failed to post GitHub review comment', error);
      vscode.window.showErrorMessage(`Failed to post review comment: ${String(error)}`);
    }
  }

  private markCommentAsPosted(
    comment: ReviewComment,
    created: CreatedInlineComment | void,
    fallbackBody: string,
    context: ReviewContext,
  ): void {
    comment.body = reviewCommentMarkdown(
      created?.body || fallbackBody,
      created?.url,
      this.reviewStackUrl(context),
    );
    comment.remoteId = created?.id;
    comment.author = {
      name: created?.author || 'You',
      iconPath:
        created?.authorAvatarUri == null ? undefined : vscode.Uri.parse(created.authorAvatarUri),
    };
    comment.timestamp = created?.created ?? new Date();
    comment.contextValue = undefined;
    comment.label = undefined;
    comment.mode = vscode.CommentMode.Preview;
    comment.parent.comments = [...comment.parent.comments];
  }

  private reviewStackUrl(context: ReviewContext): string | undefined {
    const system = context.repo.info.codeReviewSystem;
    if (context.diffId == null || system.type !== 'github') {
      return undefined;
    }
    return reviewStackPullRequestUrl(
      system.owner,
      system.repo,
      context.diffId,
      context.repo.info.pullRequestDomain,
    );
  }

  private handleSelectionChange(event: vscode.TextEditorSelectionChangeEvent): void {
    const context = this.contexts.get(event.textEditor.document.uri.toString());
    if (context == null) {
      return;
    }
    const selection = event.selections[0];
    if (selection == null) {
      return;
    }

    if (!selection.isEmpty) {
      context.activeRange = new vscode.Range(selection.start.line, 0, selection.end.line, 0);
      this.updateActiveRangeDecorations();
      // VS Code owns empty comment threads and does not expose a creation event. Collapsing the
      // previous thread during a new drag prevents abandoned editors from accumulating.
      void vscode.commands.executeCommand('workbench.action.collapseAllComments');
      return;
    }

    const threadAtCursor = [...this.draftThreads, ...context.remoteThreads].find(
      thread =>
        thread.uri.toString() === context.uri.toString() &&
        selection.active.line >= thread.range.start.line &&
        selection.active.line <= thread.range.end.line,
    );
    if (threadAtCursor != null) {
      context.activeRange = threadAtCursor.range;
      this.updateActiveRangeDecorations();
      return;
    }

    // VS Code collapses a gutter drag selection to the final line before it creates the comment
    // thread. Keep the captured range until the user moves elsewhere or cancels the comment.
    if (context.activeRange?.end.line === selection.active.line) {
      return;
    }
    context.activeRange = undefined;
    this.updateActiveRangeDecorations();
    void vscode.commands.executeCommand('workbench.action.collapseAllComments');
  }

  private setActiveRange(thread: vscode.CommentThread): void {
    const context = this.contexts.get(thread.uri.toString());
    if (context != null) {
      context.activeRange = thread.range;
      this.updateActiveRangeDecorations();
    }
  }

  private clearActiveRange(thread: vscode.CommentThread): void {
    const context = this.contexts.get(thread.uri.toString());
    if (context != null && rangesEqual(context.activeRange, thread.range)) {
      context.activeRange = undefined;
      this.updateActiveRangeDecorations();
    }
  }

  private updateActiveRangeDecorations(): void {
    for (const editor of vscode.window.visibleTextEditors) {
      const context = this.contexts.get(editor.document.uri.toString());
      editor.setDecorations(
        this.activeRangeDecoration,
        context?.activeRange == null ? [] : [context.activeRange],
      );
    }
  }

  private markCommentAsFailed(comment: ReviewComment): void {
    comment.mode = vscode.CommentMode.Editing;
    comment.contextValue = 'saplingFailedDraft';
    comment.label = 'Not posted';
    comment.parent.contextValue = 'saplingFailedDraft';
    comment.parent.canReply = false;
    comment.parent.comments = [...comment.parent.comments];
  }

  dispose(): void {
    clearInterval(this.refreshTimer);
    for (const context of this.contexts.values()) {
      for (const thread of context.remoteThreads) {
        thread.dispose();
      }
    }
    this.disposables.forEach(disposable => disposable.dispose());
    this.activeRangeDecoration.dispose();
    this.controller.dispose();
    this.commentFetchCache.clear();
    this.contexts.clear();
    this.draftThreads.clear();
  }
}

function reviewCommitHash(comparison: Comparison): string | undefined {
  switch (comparison.type) {
    case ComparisonType.Committed:
    case ComparisonType.SinceLastCodeReviewSubmit:
      return comparison.hash;
    default:
      return undefined;
  }
}

export function selectedLineText(document: vscode.TextDocument, range: vscode.Range): string {
  const lines = [];
  for (let line = range.start.line; line <= range.end.line; line++) {
    lines.push(document.lineAt(line).text);
  }
  return lines.join('\n');
}

export function suggestionBody(selectedLines: string, existingComment = ''): string {
  const prefix = existingComment.trim();
  return `${prefix}${prefix === '' ? '' : '\n\n'}\`\`\`suggestion\n${selectedLines}\n\`\`\``;
}

export function reviewStackPullRequestUrl(
  owner: string,
  repo: string,
  pullRequest: string,
  domain = 'https://reviewstack.dev',
): string {
  const baseUrl = domain.startsWith('http') ? domain : `https://${domain}`;
  return `${baseUrl.replace(/\/$/, '')}/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/pull/${encodeURIComponent(pullRequest)}`;
}

export function reviewCommentBody(
  body: string,
  remoteUrl?: string,
  reviewStackUrl?: string,
): string {
  const links = [
    remoteUrl == null ? undefined : `[View on GitHub](${remoteUrl})`,
    reviewStackUrl == null ? undefined : `[View in ReviewStack](${reviewStackUrl})`,
  ].filter(link => link != null);
  return links.length === 0 ? body : `${links.join(' · ')}\n\n${body}`;
}

function reviewCommentMarkdown(
  body: string,
  remoteUrl?: string,
  reviewStackUrl?: string,
): vscode.MarkdownString {
  const markdown = new vscode.MarkdownString(reviewCommentBody(body, remoteUrl, reviewStackUrl));
  markdown.isTrusted = false;
  return markdown;
}

function rangesEqual(left: vscode.Range | undefined, right: vscode.Range): boolean {
  return (
    left != null &&
    left.start.line === right.start.line &&
    left.start.character === right.start.character &&
    left.end.line === right.end.line &&
    left.end.character === right.end.character
  );
}

function commentBody(comment: ReviewComment): string {
  return typeof comment.body === 'string' ? comment.body : comment.body.value;
}

let activeProvider: GitHubReviewCommentsProvider | undefined;

export function registerGitHubReviewCommentsProvider(ctx: RepositoryContext): vscode.Disposable {
  activeProvider?.dispose();
  const provider = new GitHubReviewCommentsProvider(ctx);
  activeProvider = provider;
  return {
    dispose: () => {
      provider.dispose();
      if (activeProvider === provider) {
        activeProvider = undefined;
      }
    },
  };
}

export function trackGitHubReviewDiff(
  uri: vscode.Uri,
  fileUri: vscode.Uri,
  comparison: Comparison,
): void {
  activeProvider?.track(uri, fileUri, comparison);
}
