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
import {Icon} from 'shared/Icon';

import './SplitDiffHunk.css';

export function FileHeader({
  path,
  copyFrom,
  diffType,
  open,
  onChangeOpen,
  fileActions,
}: {
  path: RepoPath;
  copyFrom?: RepoPath;
  diffType?: DiffType;
  fileActions?: JSX.Element;
} & EnsureAssignedTogether<{
  open: boolean;
  onChangeOpen: (open: boolean) => void;
}>) {
  // Even though the enclosing <SplitDiffView> will have border-radius set, we
  // have to define it again here or things don't look right.
  const color =
    diffType === undefined ? 'var(--scm-modified-foreground)' : diffTypeToColor[diffType];

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
  if (copyFrom != null) {
    const copyFromParts = copyFrom.split(pathSeparator);
    commonPrefixLen = commonPrefixLength(pathParts, copyFromParts);
    copyFromRest = copyFromParts.slice(commonPrefixLen).join(pathSeparator);
  }

  const copySpan = (s: string) => <span className="file-header-copyfrom-path">{s}</span>;
  const filePathParts = pathParts.map((part, idx) => {
    const pathSoFar = pathParts.slice(idx).join(pathSeparator);
    let copyFromLeft = null;
    let copyFromRight = null;
    if (idx === commonPrefixLen) {
      // Insert "{" (when commonPrefix is not empty), " copyFromRest ->".
      const prefix = commonPrefixLen > 0 ? '{ ' : '';
      copyFromLeft = (
        <Tooltip
          title={t('Renamed or copied from $path', {replace: {$path: copyFrom ?? ''}})}
          delayMs={100}
          placement="bottom"
          key="copy">
          {copySpan(`${prefix}${copyFromRest} →`)}
        </Tooltip>
      );
    }
    if (idx + 1 === pathParts.length && commonPrefixLen > 0) {
      // Append "}" (when commonPrefix is not empty)
      copyFromRight = copySpan('}');
    }
    return (
      <>
        {copyFromLeft}
        <span className={'file-header-copyable-path'} key={idx}>
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
      </>
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
      {diffType !== undefined && <Icon icon={diffTypeToIcon[diffType]} />}
      <div className="split-diff-view-file-path-parts">{filePathParts}</div>
      {fileActions}
    </div>
  );
}

const diffTypeToColor: Record<keyof typeof DiffType, string> = {
  Modified: 'var(--scm-modified-foreground)',
  Added: 'var(--scm-added-foreground)',
  Removed: 'var(--scm-removed-foreground)',
  Renamed: 'var(--scm-modified-foreground)',
  Copied: 'var(--scm-added-foreground)',
};

const diffTypeToIcon: Record<keyof typeof DiffType, string> = {
  Modified: 'diff-modified',
  Added: 'diff-added',
  Removed: 'diff-removed',
  Renamed: 'diff-renamed',
  Copied: 'diff-renamed',
};

function commonPrefixLength<T>(a: Array<T>, b: Array<T>): number {
  let i = 0;
  while (i < a.length && i < b.length && a[i] === b[i]) {
    i++;
  }
  return i;
}
