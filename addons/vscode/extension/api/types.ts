/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type * as vscode from 'vscode';

/*
 * Note: this file is intended to be self contained so it may be copied/referenced from other extensions.
 */

/**
 * This API is exported from the meta.sapling-scm vscode extension.
 * It allows other vscode extensions to interact with Sapling & ISL.
 *
 * Usage:
 * ```
 * const api = await vscode.extensions.getExtension('meta.sapling-scm')?.activate();
 * const repo = api?.getRepositoryForPath(cwd);
 * const currentCommit = repo?.getDotCommit();
 * const currentChanges = repo?.getUncommittedChanges();
 * ```
 */
export interface SaplingExtensionApi {
  version: '1';

  getActiveRepositories(): SaplingRepository[];
  onDidChangeActiveRepositories(
    callback: (repositories: SaplingRepository[]) => void,
  ): vscode.Disposable;

  getRepositoryForPath(path: string): SaplingRepository | undefined;
}

export type SaplingRepositoryInfo = {
  type: 'success';
  repoRoot: string;
  codeReviewSystem:
    | {
        type: 'github';
        owner: string;
        repo: string;
        /** github enterprise may use a different hostname than 'github.com' */
        hostname: string;
      }
    | {
        type: 'phabricator';
        repo: string;
        callsign?: string;
      }
    | {
        type: 'none';
      }
    | {
        type: 'unknown';
        path?: string;
      };
};

export interface SaplingRepository {
  info: SaplingRepositoryInfo;

  /**
   * Run a Sapling command in this repo.
   * `runSlCommand(['status'])` is equivalent to running `sl status` in the terminal.
   *
   * Generally, this should be used for read-only non-mutating commands (status, log, blame, ...),
   * and not mutating operations (pull, commit, rebase, ...),
   * in order to get queueing support and to show progress in the UI.
   */
  runSlCommand(args: Array<string>): Promise<SaplingCommandOutput>;

  /**
   * Get the current commit ('.' revset) for this repo. This is cached from the last time it was requested.
   *
   */
  getDotCommit(): SaplingCommitInfo | undefined;
  /**
   * Subscribe to changes to the current commit ('.' revset) for this repo.
   */
  onChangeDotCommit(callback: (commit: SaplingCommitInfo | undefined) => void): vscode.Disposable;

  getUncommittedChanges(): ReadonlyArray<SaplingChangedFile>;
  onChangeUncommittedChanges(
    callback: (changes: ReadonlyArray<SaplingChangedFile>) => void,
  ): vscode.Disposable;

  /**
   * Get the current stack of commits.
   *
   * Ordered from newest to oldest, with the current commit at the front.
   */
  getCurrentStack(): Promise<ReadonlyArray<SaplingCommitInfo>>;

  /**
   *
   * Get the diff for the specified commit. If not provided, get the diff for the current commit.
   */
  getDiff(commit?: string): Promise<string>;

  // TODO: refresh
  // TODO: moveFile / copyFile
  // TODO: run operations (commit, amend, discard, purge, rebase, pull, ...)
  // TODO: get latest commit message from code review provider
}

type RepoRelativePath = string;
export type SaplingCommitInfo = {
  title: string;
  hash: string;
  author: string;
  date: Date;
  /**
   * This matches the "parents" information from source control without the
   * "null" hash. Most of the time a commit has 1 parent. For merges there
   * could be 2 or more parents. The initial commit (and initial commits of
   * other merged-in repos) have no parents.
   */
  parents: ReadonlyArray<string>;
  phase: 'public' | 'draft';
  /**
   * Whether this commit is the "." (working directory parent).
   * It is the parent of "wdir()" or the "You are here" virtual commit.
   */
  isDot: boolean;
  /** Simple string commit message. Use parsed commit messages to find specific fields. */
  description: string;
  bookmarks: ReadonlyArray<string>;
  remoteBookmarks: ReadonlyArray<string>;
  /** First few file paths changed in this commit (it's a subset for performance). Empty for public commits. */
  filePathsSample: ReadonlyArray<RepoRelativePath>;
  /** Total number of changed files in this commit. */
  totalFileCount: number;
  /** Diff number or pull request number for this commit, if applicable. */
  diffId?: string;
};

export type SaplingChangedFile = {
  path: RepoRelativePath;
  status: 'A' | 'M' | 'R' | '?' | '!' | 'U' | 'Resolved';
  /**
   * If this file is copied from another, this is the path of the original file
   * If this file is renamed from another, this is the path of the original file, and another change of type 'R' will exist.
   * */
  copy?: RepoRelativePath;
};

export type SaplingCommandOutput = {
  stdout: string;
  stderr: string;
  exitCode: number;
  killed?: boolean;
};
