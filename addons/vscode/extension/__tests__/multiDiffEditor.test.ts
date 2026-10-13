/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ChangedFile} from 'isl/src/types';

import {ComparisonType} from 'shared/Comparison';
import * as vscode from 'vscode';
import {openMultiDiffEditor} from '../multiDiffEditor';

type MockTab = {label: string; isDirty: boolean; sourceUri: string};

describe('openMultiDiffEditor', () => {
  const repoRoot = '/path/to/repo';
  const firstHash = '1111111111111111111111111111111111111111';
  const secondHash = '2222222222222222222222222222222222222222';
  const headChanges = {type: ComparisonType.HeadChanges} as const;
  const uncommittedChanges = {type: ComparisonType.UncommittedChanges} as const;
  let tabs: Array<MockTab>;

  beforeEach(() => {
    tabs = [];
    (vscode.commands.getCommands as jest.Mock).mockResolvedValue([
      '_workbench.openMultiDiffEditor',
    ]);
    // Like VS Code, reuse an open tab with the same multiDiffSourceUri instead of opening a new one,
    // and label tabs with the file count.
    (vscode.commands.executeCommand as jest.Mock).mockImplementation(
      (
        _command: string,
        args: {title: string; multiDiffSourceUri: vscode.Uri; resources: Array<unknown>},
      ) => {
        const sourceUri = args.multiDiffSourceUri.toString();
        let openedTab = tabs.find(tab => tab.sourceUri === sourceUri);
        if (openedTab == null) {
          const label = `${args.title} (${args.resources.length} files)`;
          openedTab = {label, isDirty: false, sourceUri};
          tabs.push(openedTab);
        }
        return Promise.resolve();
      },
    );
    (vscode.window as {tabGroups: unknown}).tabGroups = {
      get all() {
        return [{tabs}];
      },
      close: jest.fn((tabsToClose: MockTab | Array<MockTab>) => {
        const closing = new Set(Array.isArray(tabsToClose) ? tabsToClose : [tabsToClose]);
        tabs = tabs.filter(tab => !closing.has(tab));
        return Promise.resolve(true);
      }),
    };
  });

  const files = (...paths: Array<string>): Array<ChangedFile> =>
    paths.map(path => ({path, status: 'M'}));

  const sourceUriOfLastOpen = (): vscode.Uri =>
    (vscode.commands.executeCommand as jest.Mock).mock.lastCall[1].multiDiffSourceUri;

  const titleOfLastOpen = (): string =>
    (vscode.commands.executeCommand as jest.Mock).mock.lastCall[1].title;

  it('uses the comparison commit hash instead of the changed files as the key', async () => {
    const comparison = {type: ComparisonType.Committed, hash: firstHash} as const;
    await openMultiDiffEditor(repoRoot, comparison, files('a.txt'));
    const firstSourceUri = sourceUriOfLastOpen().toString();
    await openMultiDiffEditor(repoRoot, comparison, files('a.txt', 'b.txt'));

    expect(sourceUriOfLastOpen().path).toEqual(`${repoRoot}/Commit:${firstHash}`);
    expect(sourceUriOfLastOpen().toString()).toEqual(firstSourceUri);
    expect(tabs).toHaveLength(1);
  });

  it('uses a different key for a different commit hash', async () => {
    await openMultiDiffEditor(
      repoRoot,
      {type: ComparisonType.Committed, hash: firstHash},
      files('a.txt'),
    );
    const firstSourceUri = sourceUriOfLastOpen().toString();
    await openMultiDiffEditor(
      repoRoot,
      {type: ComparisonType.Committed, hash: secondHash},
      files('a.txt'),
    );

    expect(sourceUriOfLastOpen().toString()).not.toEqual(firstSourceUri);
    expect(sourceUriOfLastOpen().path).toEqual(`${repoRoot}/Commit:${secondHash}`);
  });

  it('includes head in the key and hash in the title for a head commit', async () => {
    await openMultiDiffEditor(repoRoot, headChanges, files('a.txt'), firstHash);

    expect(sourceUriOfLastOpen().path).toEqual(`${repoRoot}/Head`);
    expect(titleOfLastOpen()).toEqual('Head Changes [111111111111]');
  });

  it('includes uncommitted in the key and hash in the title for uncommitted changes', async () => {
    await openMultiDiffEditor(repoRoot, uncommittedChanges, files('a.txt'), firstHash);

    expect(sourceUriOfLastOpen().path).toEqual(`${repoRoot}/Uncommitted`);
    expect(titleOfLastOpen()).toEqual('Uncommitted Changes [111111111111]');
  });

  it('closes previously open head change tabs, including dirty tabs', async () => {
    await openMultiDiffEditor(repoRoot, headChanges, files('a.txt'), firstHash);
    const firstTab = tabs[0];
    firstTab.isDirty = true;
    await openMultiDiffEditor(repoRoot, headChanges, files('b.txt'), secondHash);

    expect(vscode.window.tabGroups.close).toHaveBeenCalledWith([firstTab]);
    expect(tabs).toHaveLength(1);
    expect(tabs[0]).not.toBe(firstTab);
    expect(tabs[0].sourceUri).toEqual(sourceUriOfLastOpen().toString());
  });

  it('closes previously open uncommitted change tabs, including dirty tabs', async () => {
    await openMultiDiffEditor(repoRoot, uncommittedChanges, files('a.txt'), firstHash);
    const firstTab = tabs[0];
    firstTab.isDirty = true;
    await openMultiDiffEditor(repoRoot, uncommittedChanges, files('b.txt'), secondHash);

    expect(vscode.window.tabGroups.close).toHaveBeenCalledWith([firstTab]);
    expect(tabs).toHaveLength(1);
    expect(tabs[0]).not.toBe(firstTab);
    expect(tabs[0].sourceUri).toEqual(sourceUriOfLastOpen().toString());
  });

  it('does not close an unrelated tab that shares the title prefix', async () => {
    await openMultiDiffEditor(repoRoot, headChanges, files('a.txt'), firstHash);
    const firstHeadTab = tabs[0];
    const unrelatedTab = {
      label: 'Head Changes [111111111111] notes',
      isDirty: false,
      sourceUri: 'file:///notes.txt',
    };
    tabs.push(unrelatedTab);

    await openMultiDiffEditor(repoRoot, headChanges, files('b.txt'), secondHash);

    expect(vscode.window.tabGroups.close).toHaveBeenCalledWith([firstHeadTab]);
    expect(tabs).toContain(unrelatedTab);
    expect(tabs).not.toContain(firstHeadTab);
  });

  it('does not close tabs for other comparisons', async () => {
    await openMultiDiffEditor(
      repoRoot,
      {type: ComparisonType.Committed, hash: firstHash},
      files('committed.txt'),
    );
    const committedTab = tabs[0];
    await openMultiDiffEditor(repoRoot, headChanges, files('head.txt'), firstHash);
    const headTab = tabs[1];
    await openMultiDiffEditor(repoRoot, uncommittedChanges, files('a.txt'), firstHash);
    const firstUncommittedTab = tabs[2];
    await openMultiDiffEditor(repoRoot, uncommittedChanges, files('b.txt'), secondHash);

    expect(tabs).toContain(committedTab);
    expect(tabs).toContain(headTab);
    expect(tabs).not.toContain(firstUncommittedTab);
    expect(tabs).toHaveLength(3);
  });
});
