/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ParsedDiff} from 'shared/patch/types';

import {Icon} from 'isl-components/Icon';
import {useMemo, useState} from 'react';
import {DiffType} from 'shared/patch/types';
import {T} from '../i18n';

import './FileListOverview.css';

type FileStats = {
  path: string;
  additions: number;
  deletions: number;
  fileType: 'added' | 'removed' | 'modified' | 'renamed';
};

function computeFileStats(diffs: ParsedDiff[]): FileStats[] {
  return diffs.map(diff => {
    let additions = 0;
    let deletions = 0;
    for (const hunk of diff.hunks) {
      for (const line of hunk.lines) {
        if (line.startsWith('+')) {
          additions++;
        } else if (line.startsWith('-')) {
          deletions++;
        }
      }
    }

    let fileType: FileStats['fileType'];
    if (diff.type === DiffType.Added) {
      fileType = 'added';
    } else if (diff.type === DiffType.Removed || diff.newFileName === '/dev/null') {
      fileType = 'removed';
    } else if (diff.type === DiffType.Renamed || (diff.oldFileName !== diff.newFileName && diff.oldFileName != null && diff.newFileName != null)) {
      fileType = 'renamed';
    } else {
      fileType = 'modified';
    }

    const path = diff.newFileName ?? diff.oldFileName ?? '';

    return {path, additions, deletions, fileType};
  });
}

const fileTypeToIcon: Record<FileStats['fileType'], string> = {
  added: 'diff-added',
  removed: 'diff-removed',
  modified: 'diff-modified',
  renamed: 'diff-renamed',
};

export function FileListOverview({
  diffs,
  onFileClick,
}: {
  diffs: ParsedDiff[];
  onFileClick: (path: string) => void;
}) {
  const [collapsed, setCollapsed] = useState(false);
  const fileStats = useMemo(() => computeFileStats(diffs), [diffs]);

  return (
    <div className="file-list-overview">
      <button
        className="file-list-header"
        onClick={() => setCollapsed(prev => !prev)}>
        <Icon icon={collapsed ? 'chevron-right' : 'chevron-down'} />
        <span className="file-list-header-label">
          <T>Files</T>
        </span>
        <span className="file-list-header-count">
          {fileStats.length} {fileStats.length === 1 ? 'file' : 'files'}
        </span>
      </button>
      {!collapsed && (
        <div className="file-list-items">
          {fileStats.map(file => (
            <button
              key={file.path}
              className="file-list-item"
              onClick={() => onFileClick(file.path)}>
              <Icon
                icon={fileTypeToIcon[file.fileType]}
                className={`file-list-icon-${file.fileType}`}
              />
              <span className="file-list-path" title={file.path}>
                {file.path}
              </span>
              <span className="file-list-stats">
                {file.additions > 0 && (
                  <span className="file-stat-add">+{file.additions}</span>
                )}
                {file.deletions > 0 && (
                  <span className="file-stat-del">&minus;{file.deletions}</span>
                )}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
