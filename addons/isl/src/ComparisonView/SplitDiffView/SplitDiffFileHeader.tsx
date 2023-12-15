/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {EnsureAssignedTogether} from 'shared/EnsureAssignedTogether';
import type {DiffType} from 'shared/patch/parse';
import type {RepoPath} from 'shared/types/common';

import {Tooltip} from '../../Tooltip';
import {t} from '../../i18n';
import platform from '../../platform';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import React from 'react';
import {Icon} from 'shared/Icon';

import './SplitDiffHunk.css';

/**
 * Decides the icon of the file header.
 * Subset of `DiffType` - no Copied for Removed.
 */
export enum IconType {
  Modified = 'Modified',
  Added = 'Added',
  Removed = 'Removed',
}

export function diffTypeToIconType(diffType?: DiffType): IconType {
  if (diffType != null) {
    const diffTypeStr = diffType as string;
    if (diffTypeStr === 'Added' || diffTypeStr === 'Removed' || diffTypeStr === 'Modified') {
      return diffTypeStr as IconType;
    }
  }
  // "Copied" and "Renamed" should only apply to new files.
  return IconType.Added;
}

export function FileHeader({
  path,
  copyFrom,
  iconType,
  open,
  onChangeOpen,
  fileActions,
}: {
  path: RepoPath;
  copyFrom?: RepoPath;
  iconType?: IconType;
  fileActions?: JSX.Element;
} & EnsureAssignedTogether<{
  open: boolean;
  onChangeOpen: (open: boolean) => void;
}>) {
  // Even though the enclosing <SplitDiffView> will have border-radius set, we
  // have to define it again here or things don't look right.
  const color =
    iconType === undefined ? 'var(--scm-modified-foreground)' : iconTypeToColor[iconType];

  const pathSeparator = '/';
  const pathParts = path.split(pathSeparator);

  // show: dir1 / dir2 / ... / dir9 / file
  //       ^^^^                ^^^^   ^^^^
  // copy: full path    "dir9/file"   "file"

  // with copyFrom: "dir1/dir2/dir3/foo" renamed to "dir1/dir2/dir4/bar"
  // show: dir1 / dir2 / { dir3 / foo -> dir4 / bar }
  // commonPrefixLen = 2 # (dir1 / dir2)
  // copyFromRest = "dir3/foo"
  let commonPrefixLen = -1;
  let copyFromRest = '';
  if (copyFrom != null && copyFrom !== path) {
    const copyFromParts = copyFrom.split(pathSeparator);
    commonPrefixLen = commonPrefixLength(pathParts, copyFromParts);
    copyFromRest = copyFromParts.slice(commonPrefixLen).join(pathSeparator);
  }

  const copySpan = (s: string) => <span className="file-header-copyfrom-path">{s}</span>;
  const filePathParts = pathParts.map((part, idx) => {
    const pathSoFar = pathParts.slice(idx).join(pathSeparator);
    let copyFromLeft = null;
    let copyFromRight = null;
    if (idx === commonPrefixLen && copyFromRest.length > 0) {
      // Insert "{" (when commonPrefix is not empty), " copyFromRest ->".
      const prefix = commonPrefixLen > 0 ? '{ ' : '';
      copyFromLeft = (
        <Tooltip
          title={t('Renamed or copied from $path', {replace: {$path: copyFrom ?? ''}})}
          delayMs={100}
          placement="bottom">
          {copySpan(`${prefix}${copyFromRest} →`)}
        </Tooltip>
      );
    }
    if (idx + 1 === pathParts.length && commonPrefixLen > 0 && copyFromRest.length > 0) {
      // Append "}" (when commonPrefix is not empty)
      copyFromRight = copySpan('}');
    }
    return (
      <React.Fragment key={idx}>
        {copyFromLeft}
        <span className={'file-header-copyable-path'}>
          <Tooltip
            component={() => (
              <span className="file-header-copyable-path-hover">
                {t('Copy $path', {replace: {$path: pathSoFar}})}
              </span>
            )}
            delayMs={100}
            placement="bottom">
            <span onClick={() => platform.clipboardCopy(pathSoFar)}>
              {part}
              {idx < pathParts.length - 1 ? pathSeparator : ''}
            </span>
          </Tooltip>
        </span>
        {copyFromRight}
      </React.Fragment>
    );
  });

  return (
    <div
      className={`split-diff-view-file-header file-header-${open ? 'open' : 'collapsed'}`}
      style={{color}}>
      {onChangeOpen && (
        <VSCodeButton
          appearance="icon"
          className="split-diff-view-file-header-open-button"
          data-testid={`split-diff-view-file-header-${open ? 'collapse' : 'expand'}-button`}
          onClick={() => onChangeOpen(!open)}>
          <Icon icon={open ? 'chevron-down' : 'chevron-right'} />
        </VSCodeButton>
      )}
      {iconType !== undefined && (
        <Tooltip title={iconTypeToTooltip[iconType]}>
          <Icon icon={iconTypeToIcon[iconType]} />
        </Tooltip>
      )}
      <div className="split-diff-view-file-path-parts">{filePathParts}</div>
      {fileActions}
    </div>
  );
}

const iconTypeToColor: Record<keyof typeof IconType, string> = {
  Modified: 'var(--scm-modified-foreground)',
  Added: 'var(--scm-added-foreground)',
  Removed: 'var(--scm-removed-foreground)',
};

const iconTypeToIcon: Record<keyof typeof IconType, string> = {
  Modified: 'diff-modified',
  Added: 'diff-added',
  Removed: 'diff-removed',
};

const iconTypeToTooltip: Record<keyof typeof IconType, string> = {
  Modified: t('This file was modified.'),
  Added: t('This file was added.'),
  Removed: t('This file was removed.'),
};

function commonPrefixLength<T>(a: Array<T>, b: Array<T>): number {
  let i = 0;
  while (i < a.length && i < b.length && a[i] === b[i]) {
    i++;
  }
  return i;
}
