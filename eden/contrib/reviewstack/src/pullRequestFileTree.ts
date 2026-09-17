/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DisplayChange} from './coalesceRenamedFiles';

import {getDisplayChangePath} from './diffFileNavigation';

export type PullRequestFileTreeFile = {
  kind: 'file';
  change: DisplayChange;
  name: string;
  path: string;
};

export type PullRequestFileTreeDirectory = {
  kind: 'directory';
  children: PullRequestFileTreeNode[];
  name: string;
  path: string;
};

export type PullRequestFileTreeNode =
  | PullRequestFileTreeDirectory
  | PullRequestFileTreeFile;

type MutableDirectory = {
  directories: Map<string, MutableDirectory>;
  files: PullRequestFileTreeFile[];
};

function compareNodes(a: PullRequestFileTreeNode, b: PullRequestFileTreeNode): number {
  if (a.kind !== b.kind) {
    return a.kind === 'directory' ? -1 : 1;
  }
  return a.name.localeCompare(b.name);
}

export default function buildPullRequestFileTree(
  changes: DisplayChange[],
): PullRequestFileTreeNode[] {
  const root: MutableDirectory = {directories: new Map(), files: []};

  for (const change of changes) {
    const path = getDisplayChangePath(change);
    const parts = path.split('/').filter(Boolean);
    const name = parts.pop();
    if (name == null) {
      continue;
    }

    let directory = root;
    for (const part of parts) {
      let child = directory.directories.get(part);
      if (child == null) {
        child = {directories: new Map(), files: []};
        directory.directories.set(part, child);
      }
      directory = child;
    }
    directory.files.push({kind: 'file', change, name, path});
  }

  function finish(directory: MutableDirectory, parentPath: string): PullRequestFileTreeNode[] {
    const nodes: PullRequestFileTreeNode[] = [];
    for (const [name, child] of directory.directories) {
      const path = parentPath === '' ? name : `${parentPath}/${name}`;
      nodes.push({kind: 'directory', children: finish(child, path), name, path});
    }
    nodes.push(...directory.files);
    return nodes.sort(compareNodes);
  }

  return finish(root, '');
}
