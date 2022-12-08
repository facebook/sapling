/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Repository} from 'isl-server/src/Repository';
import type {Logger} from 'isl-server/src/logger';
import type {Disposable} from 'isl/src/types';
import type {Comparison} from 'shared/Comparison';

import {repositoryCache} from 'isl-server/src/RepositoryCache';
import {revsetForComparison} from 'shared/Comparison';
import {LRU} from 'shared/LRU';
import {TypedEventEmitter} from 'shared/TypedEventEmitter';
import {ensureTrailingPathSep} from 'shared/pathUtils';
import * as vscode from 'vscode';

/**
 * VSCode's Quick Diff provider systems works by allowing you to describe the equivalent "original" URI
 * for the left side of a diff as a new URI with a custom scheme,
 * then you add a content provider for that custom URI to give the "original" file contents.
 *
 * SaplingDiffContentProvider uses a repository to provide the "original"
 * file content for a comparison using `sl cat`.
 */
export class SaplingDiffContentProvider implements vscode.TextDocumentContentProvider {
  private disposables: Array<vscode.Disposable> = [];

  /**
   * VS Code doesn't tell us which uris are currently open, so we need to remember
   * uris we've seen before. This is needed so we can tell VS Code which URIs are
   * invalidated when the repository changes via this.onDidChange.
   */
  private activeUrisByRepo: Map<
    Repository | 'unknown',
    Set<string /* serialized SaplingDiffEncodedUri */>
  > = new Map();

  /**
   * VS Code requests content for uris each time the diff view is focused.
   * Diff original content won't change until the current commit is changed,
   * so we can cache file contents to avoid repeat `sl cat` calls.
   * We don't want to store unlimited file contents in memory, so we use an LRU cache.
   * Missing the cache just means re-running `sl cat` again.
   */
  private fileContentsByEncodedUri = new LRU<string, string>(20);

  constructor(private logger: Logger) {
    let subscriptions: Array<Disposable> = [];
    repositoryCache.onChangeActiveRepos(activeRepos => {
      const knownRoots = activeRepos.map(repo => repo.info.repoRoot);

      // when we add a repo, we need to see if we can now provide changes to any previously requested path
      const unownedComparisons = this.activeUrisByRepo.get('unknown');
      if (unownedComparisons) {
        const fixed = [];
        for (const encoded of unownedComparisons.values()) {
          const encodedUri = vscode.Uri.parse(encoded);
          const {fsPath} = decodeSaplingDiffUri(encodedUri).originalUri;
          for (const root of knownRoots) {
            if (fsPath === root || fsPath.startsWith(ensureTrailingPathSep(root))) {
              fixed.push(encodedUri);
              break;
            }
          }
        }
        for (const change of fixed) {
          unownedComparisons.delete(change.toString());
        }
        for (const change of fixed) {
          this.changeEmitter.emit('change', change);
        }
      }

      subscriptions.forEach(sub => sub.dispose());
      subscriptions = activeRepos.map(repo =>
        // Whenever the head commit changes, it means our comparisons in that repo are no longer valid,
        // for example checking out a new commit or making a new commit changes the comparison.
        // TODO: this is slightly wastefully un- and re-subscribing to all repos whenever any of them change.
        // However, repos changing is relatively rare.
        repo.subscribeToHeadCommit(() => {
          // Clear out file cache, so all future fetches re-check with sl cat.
          // TODO: This is slightly over-aggressive, since it invalidates other repos too.
          // We could instead iterate the cache to delete paths belonging to this repo
          this.fileContentsByEncodedUri.clear();
          const uris = this.activeUrisByRepo.get(repo);
          if (uris) {
            this.logger.info(
              `head commit changed for ${repo.info.repoRoot}, invalidating ${uris.size} diff view contents`,
            );
            for (const uri of uris.values()) {
              // notify vscode of the change, so it re-runs provideTextDocumentContent
              this.changeEmitter.emit('change', vscode.Uri.parse(uri));
            }
          }
        }),
      );
      this.disposables.push({dispose: () => subscriptions.forEach(sub => sub.dispose())});
    });

    this.disposables.push(
      // track closing diff providers to know when you remove from tracked uris
      vscode.workspace.onDidCloseTextDocument(e => {
        if (e.uri.scheme === SAPLING_DIFF_PROVIDER_SCHEME) {
          for (const uris of this.activeUrisByRepo.values()) {
            const encodedUri = e.uri.toString();
            if (uris.has(encodedUri)) {
              uris.delete(encodedUri);
              // No need to clear the file content cache for this uri at this point:
              // It is very likely the user can re-open the same diff view without changing
              // their head commit. We can use cached file content between these opens
              // to avoid running `sl cat`.
            }
          }
        }
      }),
    );
  }

  private changeEmitter = new TypedEventEmitter<'change', vscode.Uri>();
  onDidChange(callback: (uri: vscode.Uri) => unknown): vscode.Disposable {
    this.changeEmitter.on('change', callback);
    return {
      dispose: () => this.changeEmitter.off('change', callback),
    };
  }

  async provideTextDocumentContent(
    encodedUri: vscode.Uri,
    _token: vscode.CancellationToken,
  ): Promise<string | null> {
    const encodedUriString = encodedUri.toString();
    const data = decodeSaplingDiffUri(encodedUri);
    const {fsPath} = data.originalUri;

    const repo = repositoryCache.cachedRepositoryForPath(fsPath);

    // remember that this URI was requested.
    const activeUrisSet = this.activeUrisByRepo.get(repo ?? 'unknown') ?? new Set();
    activeUrisSet.add(encodedUriString);
    this.activeUrisByRepo.set(repo ?? 'unknown', activeUrisSet);

    this.logger.info('repo for path:', repo?.info.repoRoot);
    if (repo == null) {
      return null;
    }

    // try the cache first
    const cachedFileContent = this.fileContentsByEncodedUri.get(encodedUriString);
    if (cachedFileContent != null) {
      return cachedFileContent;
    }

    const revset = revsetForComparison(data.comparison);

    // fall back to fetching from the repo
    const fetchedFileContent = await repo
      .cat(fsPath, revset)
      // An error during `cat` usually means the right side of the comparison was added since the left,
      // so `cat` claims `no such file` at that revset.
      // TODO: it would be more accurate to check that the error is due to this, and return null if not.
      .catch(() => '');
    if (fetchedFileContent != null) {
      this.fileContentsByEncodedUri.set(encodedUriString, fetchedFileContent);
    }
    return fetchedFileContent;
  }

  public dispose() {
    this.disposables.forEach(d => d.dispose());
    this.disposables.length = 0;
  }
}

export function registerSaplingDiffContentProvider(logger: Logger): vscode.Disposable {
  return vscode.workspace.registerTextDocumentContentProvider(
    SAPLING_DIFF_PROVIDER_SCHEME,
    new SaplingDiffContentProvider(logger),
  );
}

export const SAPLING_DIFF_PROVIDER_SCHEME = 'sapling-diff';
/**
 * {@link vscode.Uri} with scheme of {@link SAPLING_DIFF_PROVIDER_SCHEME}
 */
type SaplingDiffEncodedUri = vscode.Uri;

type SaplingURIEncodedData = {
  comparison: Comparison;
};

/**
 * Encode a normal file's URI plus a comparison revset
 * to get the custom URI which {@link SaplingDiffContentProvider} knows how to provide content for
 * that file at that revset.
 * Decoded by {@link decodeSaplingDiffUri}.
 */
export function encodeSaplingDiffUri(
  uri: vscode.Uri,
  comparison: Comparison,
): SaplingDiffEncodedUri {
  if (uri.scheme !== 'file') {
    throw new Error('encoding non-file:// uris as sapling diff uris is not supported');
  }
  return uri.with({
    scheme: SAPLING_DIFF_PROVIDER_SCHEME,
    query: JSON.stringify({
      comparison,
    } as SaplingURIEncodedData),
  });
}

/**
 * Decode a custom URI which was encoded by  {@link encodeSaplingDiffUri},
 * to get the original file URI back.
 */
export function decodeSaplingDiffUri(uri: SaplingDiffEncodedUri): {
  originalUri: vscode.Uri;
  comparison: Comparison;
} {
  const data = JSON.parse(uri.query) as SaplingURIEncodedData;
  return {
    comparison: data.comparison,
    originalUri: uri.with({scheme: 'file', query: ''}),
  };
}
