/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepositoryReference} from 'isl-server/src/RepositoryCache';
import type {Logger} from 'isl-server/src/logger';
import type {ChangedFile} from 'isl/src/types';
import type {Comparison} from 'shared/Comparison';
import type {Writable} from 'shared/typeUtils';

import {encodeSaplingDiffUri} from './DiffContentProvider';
import {getCLICommand} from './config';
import {t} from './i18n';
import {Repository} from 'isl-server/src/Repository';
import {repositoryCache} from 'isl-server/src/RepositoryCache';
import {ComparisonType} from 'shared/Comparison';
import * as vscode from 'vscode';

/**
 * Construct Repositories and VSCodeRepos for every workspace folder.
 * Treats repositoryCache as the source of truth for re-using repositories.
 */
export function watchAndCreateRepositoriesForWorkspaceFolders(logger: Logger): vscode.Disposable {
  const knownRepos = new Map<string, RepositoryReference>();
  const vscodeRepos = new Map<string, VSCodeRepo>();
  function updateRepos(
    added: ReadonlyArray<vscode.WorkspaceFolder>,
    removed: ReadonlyArray<vscode.WorkspaceFolder>,
  ) {
    for (const add of added) {
      const {fsPath} = add.uri;
      if (knownRepos.has(fsPath)) {
        throw new Error(`Attempted to add workspace folder path twice: ${fsPath}`);
      }
      const repoReference = repositoryCache.getOrCreate(getCLICommand(), logger, fsPath);
      knownRepos.set(fsPath, repoReference);
      repoReference.promise.then(repo => {
        if (repo instanceof Repository) {
          const root = repo?.info.repoRoot;
          const existing = vscodeRepos.get(root);
          if (existing) {
            return;
          }
          const vscodeRepo = new VSCodeRepo(repo, logger);
          vscodeRepos.set(root, vscodeRepo);
          repo.onDidDispose(() => {
            vscodeRepo.dispose();
            vscodeRepos.delete(root);
          });
        }
      });
    }
    for (const remove of removed) {
      const {fsPath} = remove.uri;
      const repo = knownRepos.get(fsPath);
      repo?.unref();
      knownRepos.delete(fsPath);
    }
  }
  if (vscode.workspace.workspaceFolders) {
    updateRepos(vscode.workspace.workspaceFolders, []);
  }
  return vscode.workspace.onDidChangeWorkspaceFolders(e => {
    updateRepos(e.added, e.removed);
  });
  // TODO: consider also listening for vscode.workspace.onDidOpenTextDocument to support repos
  // for ad-hoc non-workspace-folder files
}

/**
 * vscode-API-compatible repository.
 * This handles vscode-api integrations, but defers to Repository for any actual work.
 */
export class VSCodeRepo implements vscode.QuickDiffProvider {
  private disposables: Array<vscode.Disposable> = [];
  private sourceControl: vscode.SourceControl;
  private resourceGroups: Record<
    'changes' | 'untracked' | 'unresolved' | 'resolved',
    vscode.SourceControlResourceGroup
  >;
  private rootUri: vscode.Uri;

  constructor(public repo: Repository, private logger: Logger) {
    repo.onDidDispose(() => this.dispose());
    this.rootUri = vscode.Uri.file(repo.info.repoRoot);

    this.sourceControl = vscode.scm.createSourceControl(
      'sapling',
      t('Sapling'),
      vscode.Uri.file(repo.info.repoRoot),
    );
    this.sourceControl.quickDiffProvider = this;
    this.sourceControl.inputBox.enabled = false;
    this.sourceControl.inputBox.visible = false;
    this.resourceGroups = {
      changes: this.sourceControl.createResourceGroup('changes', t('Uncommitted Changes')),
      untracked: this.sourceControl.createResourceGroup('untracked', t('Untracked Changes')),
      unresolved: this.sourceControl.createResourceGroup(
        'unresolved',
        t('Unresolved Merge Conflicts'),
      ),
      resolved: this.sourceControl.createResourceGroup('resolved', t('Resolved Merge Conflicts')),
    };
    for (const group of Object.values(this.resourceGroups)) {
      group.hideWhenEmpty = true;
    }

    this.disposables.push(
      repo.subscribeToUncommittedChanges(() => {
        this.updateResourceGroups();
      }),
      repo.onChangeConflictState(() => {
        this.updateResourceGroups();
      }),
    );
    this.updateResourceGroups();
  }

  private updateResourceGroups() {
    const data = this.repo.getUncommittedChanges();
    const conflicts = this.repo.getMergeConflicts()?.files;

    // only show merge conflicts if they are given
    const fileChanges = conflicts ?? data ?? [];

    const changes: Array<vscode.SourceControlResourceState> = [];
    const untracked: Array<vscode.SourceControlResourceState> = [];
    const unresolved: Array<vscode.SourceControlResourceState> = [];
    const resolved: Array<vscode.SourceControlResourceState> = [];

    for (const change of fileChanges) {
      const uri = vscode.Uri.joinPath(this.rootUri, change.path);
      const resource: vscode.SourceControlResourceState = {
        command: {
          command: 'vscode.open',
          title: 'Open',
          arguments: [uri],
        },
        resourceUri: uri,
        decorations: this.decorationForChange(change),
      };
      switch (change.status) {
        case '?':
        case '!':
          untracked.push(resource);
          break;
        case 'U':
          unresolved.push(resource);
          break;
        case 'Resolved':
          resolved.push(resource);
          break;
        default:
          changes.push(resource);
          break;
      }
    }
    this.resourceGroups.changes.resourceStates = changes;
    this.resourceGroups.untracked.resourceStates = untracked;
    this.resourceGroups.unresolved.resourceStates = unresolved;
    this.resourceGroups.resolved.resourceStates = resolved;

    // don't include resolved files in count
    this.sourceControl.count = changes.length + untracked.length + unresolved.length;
  }

  public dispose() {
    this.disposables.forEach(d => d?.dispose());
  }

  private decorationForChange(change: ChangedFile): vscode.SourceControlResourceDecorations {
    const decoration: Writable<vscode.SourceControlResourceDecorations> = {};
    switch (change.status) {
      case 'M':
        decoration.iconPath = new vscode.ThemeIcon('diff-modified', themeColors.modified);
        break;
      case 'A':
        decoration.iconPath = new vscode.ThemeIcon('diff-added', themeColors.added);
        break;
      case 'R':
        decoration.iconPath = new vscode.ThemeIcon('diff-removed', themeColors.deleted);
        break;
      case '?':
        decoration.faded = true;
        decoration.iconPath = new vscode.ThemeIcon('question', themeColors.untracked);
        break;
      case '!':
        decoration.faded = true;
        decoration.iconPath = new vscode.ThemeIcon('warning', themeColors.untracked);
        break;
      case 'U':
        decoration.iconPath = new vscode.ThemeIcon('diff-ignored', themeColors.conflicting);
        break;
      case 'Resolved':
        decoration.faded = true;
        decoration.iconPath = new vscode.ThemeIcon('pass', themeColors.added);
        break;
      default:
        break;
    }
    return decoration;
  }

  /**
   * Use ContentProvider + encodeSaplingDiffUri
   */
  provideOriginalResource(uri: vscode.Uri): vscode.Uri | undefined {
    if (uri.scheme !== 'file') {
      return;
    }
    // TODO: make this configurable via vscode setting to allow
    // diff gutters to be either uncommitted changes / head changes / stack changes
    const comparison = {type: ComparisonType.UncommittedChanges} as Comparison;

    return encodeSaplingDiffUri(uri, comparison);
  }
}

const themeColors = {
  deleted: new vscode.ThemeColor('gitDecoration.deletedResourceForeground'),
  modified: new vscode.ThemeColor('gitDecoration.modifiedResourceForeground'),
  added: new vscode.ThemeColor('gitDecoration.addedResourceForeground'),
  untracked: new vscode.ThemeColor('gitDecoration.untrackedResourceForeground'),
  conflicting: new vscode.ThemeColor('gitDecoration.conflictingResourceForeground'),
};

export const __TEST__ = {watchAndCreateRepositoriesForWorkspaceFolders};
