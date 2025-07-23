/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Logger} from 'isl-server/src/logger';
import type {Disposable, Event} from 'vscode';
import type {SaplingResourceGroup, VSCodeRepo} from './VSCodeRepo';

import {
  EventEmitter,
  ThemeIcon,
  Uri,
  window,
  type FileDecoration,
  type FileDecorationProvider,
} from 'vscode';

export default class SaplingFileDecorationProvider implements FileDecorationProvider {
  private readonly _onDidChangeDecorations = new EventEmitter<Uri[]>();
  readonly onDidChangeFileDecorations: Event<Uri[]> = this._onDidChangeDecorations.event;

  private disposables: Disposable[] = [];
  private decorations = new Map<string, FileDecoration>();

  constructor(
    private repository: VSCodeRepo,
    private logger: Logger,
  ) {
    this.disposables.push(
      window?.registerFileDecorationProvider?.(this),
      repository.repo.subscribeToUncommittedChanges(this.onDidRunStatus.bind(this)),
      repository.repo.onChangeConflictState(this.onDidRunStatus.bind(this)),
    );
    this.onDidRunStatus();
  }

  private onDidRunStatus(): void {
    const newDecorations = new Map<string, FileDecoration>();

    const resourceGroups = this.repository.getResourceGroups() ?? {};
    for (const key of Object.keys(resourceGroups) as (keyof typeof resourceGroups)[]) {
      this.collectDecorationData(resourceGroups[key], newDecorations);
    }

    const uris = new Set([...this.decorations.keys()].concat([...newDecorations.keys()]));
    this.decorations = newDecorations;
    this._onDidChangeDecorations.fire([...uris.values()].map(value => Uri.parse(value, true)));
  }

  private collectDecorationData(
    group: SaplingResourceGroup,
    bucket: Map<string, FileDecoration>,
  ): void {
    for (const r of group.resourceStates) {
      const decoration = r.decorations;
      if (decoration) {
        bucket.set(r.resourceUri.toString(), {
          badge: r.status,
          color: decoration.iconPath instanceof ThemeIcon ? decoration.iconPath.color : undefined,
        });
      }
    }
  }

  provideFileDecoration(uri: Uri): FileDecoration | undefined {
    return this.decorations.get(uri.toString());
  }

  dispose(): void {
    this.disposables.forEach(d => d?.dispose());
  }
}
