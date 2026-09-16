/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {AddChange, CommitChange, ModifyChange, RemoveChange} from './github/diffTypes';

import {gitHubBlobAtom, gitHubPullRequestVersionDiffAtom} from './jotai';
import {getFileAnchorID, getPathForChange} from './utils';
import {
  ChevronDownIcon,
  ChevronRightIcon,
  DiffAddedIcon,
  DiffModifiedIcon,
  DiffRemovedIcon,
  FileDirectoryIcon,
  SearchIcon,
  XIcon,
} from '@primer/octicons-react';
import {Text} from '@primer/react';
import {diffLines} from 'diff';
import {useAtomValue} from 'jotai';
import React, {Suspense, useMemo, useState} from 'react';

import './PullRequestFileTree.css';

type LineStats = {
  additions: number | null;
  deletions: number | null;
};

type FileNode = {
  type: 'file';
  name: string;
  path: string;
  change: CommitChange;
};

type DirectoryNode = {
  type: 'directory';
  name: string;
  path: string;
  children: TreeNode[];
};

type TreeNode = FileNode | DirectoryNode;

const EMPTY_CHANGES: CommitChange[] = [];

export default function PullRequestFileTree(): React.ReactElement {
  const versionDiff = useAtomValue(gitHubPullRequestVersionDiffAtom);
  const changes = versionDiff?.diff ?? EMPTY_CHANGES;
  const [query, setQuery] = useState('');
  const [collapsedDirectories, setCollapsedDirectories] = useState<Set<string>>(() => new Set());
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const tree = useMemo(() => buildFileTree(changes), [changes]);
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const visibleTree = useMemo(() => filterFileTree(tree, normalizedQuery), [normalizedQuery, tree]);
  const changeCounts = useMemo(() => countChangeTypes(changes), [changes]);

  const toggleDirectory = (path: string) => {
    setCollapsedDirectories(collapsed => {
      const next = new Set(collapsed);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  };

  const selectFile = (path: string) => {
    setSelectedPath(path);
    document.getElementById(getFileAnchorID(path))?.scrollIntoView({
      behavior: 'smooth',
      block: 'start',
    });
  };

  return (
    <section className="reviewstack-file-tree" aria-labelledby="changed-files-heading">
      <header className="reviewstack-file-tree-header">
        <div className="reviewstack-file-tree-title">
          <div className="reviewstack-file-tree-title-icon" aria-hidden="true">
            <FileDirectoryIcon size={14} />
          </div>
          <Text as="h2" id="changed-files-heading" fontSize={1} fontWeight="bold">
            Changed files
          </Text>
          <span className="reviewstack-file-tree-total">{changes.length}</span>
        </div>
        {changes.length !== 0 && (
          <div className="reviewstack-file-tree-search">
            <SearchIcon size={14} aria-hidden="true" />
            <input
              type="search"
              aria-label="Filter changed files"
              value={query}
              onChange={event => setQuery(event.currentTarget.value)}
              placeholder="Filter files…"
            />
            {normalizedQuery !== '' && (
              <Text className="reviewstack-file-tree-match-count" color="fg.muted" fontSize={0}>
                {countFiles(visibleTree)}/{changes.length}
              </Text>
            )}
            {query !== '' && (
              <button type="button" onClick={() => setQuery('')} aria-label="Clear file filter">
                <XIcon size={12} />
              </button>
            )}
          </div>
        )}
        <div className="reviewstack-file-tree-summary" aria-label="Change summary">
          <ChangeTypeCount type="add" count={changeCounts.add} label="added" />
          <ChangeTypeCount type="modify" count={changeCounts.modify} label="modified" />
          <ChangeTypeCount type="remove" count={changeCounts.remove} label="removed" />
        </div>
      </header>

      {changes.length === 0 ? (
        <div className="reviewstack-file-tree-empty">
          <Text color="fg.muted" fontSize={1}>
            No changed files in this comparison.
          </Text>
        </div>
      ) : (
        <div className="reviewstack-file-tree-rows" role="tree" aria-label="Changed files">
          {visibleTree.length === 0 ? (
            <div className="reviewstack-file-tree-empty">
              <Text color="fg.muted" fontSize={1}>
                No files match “{query.trim()}”.
              </Text>
            </div>
          ) : (
            visibleTree.map(node => (
              <TreeRow
                key={`${node.type}-${node.path}`}
                node={node}
                depth={0}
                collapsedDirectories={collapsedDirectories}
                forceExpanded={normalizedQuery !== ''}
                selectedPath={selectedPath}
                onToggleDirectory={toggleDirectory}
                onSelectFile={selectFile}
              />
            ))
          )}
        </div>
      )}
    </section>
  );
}

function ChangeTypeCount({
  type,
  count,
  label,
}: {
  type: CommitChange['type'];
  count: number;
  label: string;
}): React.ReactElement | null {
  if (count === 0) {
    return null;
  }
  return (
    <span
      className={`reviewstack-file-tree-type-count reviewstack-file-tree-type-${type}`}
      aria-label={`${count} ${label}`}>
      <span aria-hidden="true">
        {statusLetter(type)} {count}
      </span>
    </span>
  );
}

function TreeRow({
  node,
  depth,
  collapsedDirectories,
  forceExpanded,
  selectedPath,
  onToggleDirectory,
  onSelectFile,
}: {
  node: TreeNode;
  depth: number;
  collapsedDirectories: Set<string>;
  forceExpanded: boolean;
  selectedPath: string | null;
  onToggleDirectory: (path: string) => void;
  onSelectFile: (path: string) => void;
}): React.ReactElement {
  if (node.type === 'file') {
    return (
      <Suspense
        fallback={
          <FileRow
            node={node}
            depth={depth}
            stats={null}
            selected={selectedPath === node.path}
            onSelect={() => onSelectFile(node.path)}
          />
        }>
        <FileRowWithStats
          node={node}
          depth={depth}
          selected={selectedPath === node.path}
          onSelect={() => onSelectFile(node.path)}
        />
      </Suspense>
    );
  }

  const collapsed = !forceExpanded && collapsedDirectories.has(node.path);
  return (
    <div role="treeitem" aria-expanded={!collapsed}>
      <button
        className="reviewstack-file-tree-directory"
        type="button"
        title={node.name}
        style={{paddingLeft: `${10 + depth * 16}px`}}
        onClick={() => onToggleDirectory(node.path)}>
        {collapsed ? (
          <ChevronRightIcon size={14} aria-hidden="true" />
        ) : (
          <ChevronDownIcon size={14} aria-hidden="true" />
        )}
        <FileDirectoryIcon size={16} aria-hidden="true" />
        <span className="reviewstack-file-tree-directory-name">{node.name}</span>
        <span className="reviewstack-file-tree-directory-count">{countFiles(node.children)}</span>
      </button>
      {!collapsed && (
        <div role="group">
          {node.children.map(child => (
            <TreeRow
              key={`${child.type}-${child.path}`}
              node={child}
              depth={depth + 1}
              collapsedDirectories={collapsedDirectories}
              forceExpanded={forceExpanded}
              selectedPath={selectedPath}
              onToggleDirectory={onToggleDirectory}
              onSelectFile={onSelectFile}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function FileRowWithStats({
  node,
  depth,
  selected,
  onSelect,
}: {
  node: FileNode;
  depth: number;
  selected: boolean;
  onSelect: () => void;
}): React.ReactElement {
  const props = {node, depth, selected, onSelect};
  switch (node.change.type) {
    case 'add':
      return <AddedFileRowWithStats {...props} change={node.change} />;
    case 'remove':
      return <RemovedFileRowWithStats {...props} change={node.change} />;
    case 'modify':
      return <ModifiedFileRowWithStats {...props} change={node.change} />;
  }
}

function FileRow({
  node,
  depth,
  stats,
  selected = false,
  onSelect,
}: {
  node: FileNode;
  depth: number;
  stats: LineStats | null;
  selected?: boolean;
  onSelect?: () => void;
}): React.ReactElement {
  const Icon = changeIcon(node.change.type);
  return (
    <button
      className={`reviewstack-file-row reviewstack-file-row-${node.change.type}${
        selected ? ' reviewstack-file-row-selected' : ''
      }`}
      type="button"
      role="treeitem"
      style={{paddingLeft: `${26 + depth * 16}px`}}
      onClick={onSelect}
      title={node.path}>
      <Icon className="reviewstack-file-icon" size={16} aria-hidden="true" />
      <span className="reviewstack-file-name">{node.name}</span>
      <span className="reviewstack-file-counts" aria-label="Line changes">
        {stats == null ? (
          <span className="reviewstack-file-counts-loading" aria-label="Loading line changes" />
        ) : (
          <>
            <span className="reviewstack-file-additions">+{formatCount(stats.additions)}</span>
            <span className="reviewstack-file-deletions">−{formatCount(stats.deletions)}</span>
          </>
        )}
      </span>
    </button>
  );
}

type FileRowWithStatsProps = {
  node: FileNode;
  depth: number;
  selected: boolean;
  onSelect: () => void;
};

function AddedFileRowWithStats({
  change,
  ...props
}: FileRowWithStatsProps & {change: AddChange}): React.ReactElement {
  const blob = useAtomValue(gitHubBlobAtom(change.entry.oid));
  const stats = {additions: countLines(blob?.text), deletions: 0};
  return <FileRow {...props} stats={stats} />;
}

function RemovedFileRowWithStats({
  change,
  ...props
}: FileRowWithStatsProps & {change: RemoveChange}): React.ReactElement {
  const blob = useAtomValue(gitHubBlobAtom(change.entry.oid));
  const stats = {additions: 0, deletions: countLines(blob?.text)};
  return <FileRow {...props} stats={stats} />;
}

function ModifiedFileRowWithStats({
  change,
  ...props
}: FileRowWithStatsProps & {change: ModifyChange}): React.ReactElement {
  const before = useAtomValue(gitHubBlobAtom(change.before.oid));
  const after = useAtomValue(gitHubBlobAtom(change.after.oid));
  if (before?.text == null || after?.text == null) {
    return <FileRow {...props} stats={{additions: null, deletions: null}} />;
  }

  const stats = diffLines(before.text, after.text).reduce(
    (stats, part) => {
      if (part.added) {
        stats.additions += part.count ?? 0;
      } else if (part.removed) {
        stats.deletions += part.count ?? 0;
      }
      return stats;
    },
    {additions: 0, deletions: 0},
  );
  return <FileRow {...props} stats={stats} />;
}

function buildFileTree(changes: CommitChange[]): TreeNode[] {
  const root: DirectoryNode = {type: 'directory', name: '', path: '', children: []};

  for (const change of changes) {
    const path = getPathForChange(change);
    const parts = path.split('/');
    const fileName = parts.pop() ?? path;
    let directory = root;

    for (const part of parts) {
      const directoryPath = directory.path === '' ? part : `${directory.path}/${part}`;
      let child = directory.children.find(
        candidate => candidate.type === 'directory' && candidate.name === part,
      ) as DirectoryNode | undefined;
      if (child == null) {
        child = {type: 'directory', name: part, path: directoryPath, children: []};
        directory.children.push(child);
      }
      directory = child;
    }

    directory.children.push({type: 'file', name: fileName, path, change});
  }

  sortFileTree(root.children);
  return compactDirectoryChains(root.children);
}

function sortFileTree(nodes: TreeNode[]): void {
  nodes.sort((left, right) => {
    if (left.type !== right.type) {
      return left.type === 'directory' ? -1 : 1;
    }
    return left.name.localeCompare(right.name);
  });
  for (const node of nodes) {
    if (node.type === 'directory') {
      sortFileTree(node.children);
    }
  }
}

function compactDirectoryChains(nodes: TreeNode[]): TreeNode[] {
  return nodes.map(node => {
    if (node.type === 'file') {
      return node;
    }

    let name = node.name;
    let path = node.path;
    let children = compactDirectoryChains(node.children);
    while (children.length === 1 && children[0].type === 'directory') {
      const child = children[0];
      name = `${name}/${child.name}`;
      path = child.path;
      children = child.children;
    }
    return {...node, name, path, children};
  });
}

function filterFileTree(nodes: TreeNode[], query: string): TreeNode[] {
  if (query === '') {
    return nodes;
  }
  return nodes.reduce<TreeNode[]>((visibleNodes, node) => {
    if (node.type === 'file') {
      if (node.path.toLocaleLowerCase().includes(query)) {
        visibleNodes.push(node);
      }
      return visibleNodes;
    }
    const children = filterFileTree(node.children, query);
    if (children.length !== 0) {
      visibleNodes.push({...node, children});
    }
    return visibleNodes;
  }, []);
}

function countFiles(nodes: TreeNode[]): number {
  return nodes.reduce(
    (total, node) => total + (node.type === 'file' ? 1 : countFiles(node.children)),
    0,
  );
}

function countChangeTypes(changes: CommitChange[]): Record<CommitChange['type'], number> {
  return changes.reduce((counts, change) => ({...counts, [change.type]: counts[change.type] + 1}), {
    add: 0,
    modify: 0,
    remove: 0,
  });
}

function changeIcon(type: CommitChange['type']) {
  switch (type) {
    case 'add':
      return DiffAddedIcon;
    case 'modify':
      return DiffModifiedIcon;
    case 'remove':
      return DiffRemovedIcon;
  }
}

function statusLetter(type: CommitChange['type']): string {
  switch (type) {
    case 'add':
      return 'A';
    case 'modify':
      return 'M';
    case 'remove':
      return 'D';
  }
}

function countLines(text: string | null | undefined): number | null {
  if (text == null) {
    return null;
  }
  if (text === '') {
    return 0;
  }
  return text.split('\n').length - (text.endsWith('\n') ? 1 : 0);
}

function formatCount(count: number | null): string {
  return count == null ? '—' : String(count);
}
