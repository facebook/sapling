/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Context} from './types';
import type {DiffType} from 'shared/patch/parse';

import {FileSymlinkFileIcon} from '@primer/octicons-react';
import {IconButton, Tooltip} from '@primer/react';
import {Icon} from 'shared/Icon';

export function FileHeader<Id>({
  ctx,
  path,
  diffType,
  open,
  onChangeOpen,
}: {
  ctx?: Context<Id>;
  path: string;
  diffType?: DiffType;
  open?: boolean;
  onChangeOpen?: (open: boolean) => void;
}) {
  // Even though the enclosing <SplitDiffView> will have border-radius set, we
  // have to define it again here or things don't look right.
  const color =
    diffType === undefined ? 'var(--scm-modified-foreground)' : diffTypeToColor[diffType];

  const pathSeparator = '/';
  const pathParts = path.split(pathSeparator);

  const t = ctx?.translate ?? (s => s);
  const copy = ctx?.copy;

  const filePathParts = (
    <>
      {pathParts.reduce((acc, part, idx) => {
        // Nest path parts in a particular way so we can use plain CSS
        // hover selectors to underline nested sub-paths.
        const pathSoFar = pathParts.slice(idx).join(pathSeparator);
        return (
          <span className={copy && 'file-header-copyable-path'} key={idx}>
            {acc}
            <Tooltip
              // TODO: better translate API that supports templates.
              aria-label={copy && t('Copy $path').replace('$path', pathSoFar)}
              direction="se"
              className="file-header-path-element">
              <span onClick={copy && (() => copy(pathSoFar))}>
                {part}
                {idx < pathParts.length - 1 ? pathSeparator : ''}
              </span>
            </Tooltip>
          </span>
        );
      }, <span />)}
    </>
  );

  return (
    <div className="split-diff-view-file-header" style={{color}}>
      {onChangeOpen && (
        <span
          className="split-diff-view-file-header-toggle-open"
          onClick={() => onChangeOpen(!open)}>
          <Icon icon={open ? 'chevron-up' : 'chevron-down'} />
        </span>
      )}
      {diffType !== undefined && <Icon icon={diffTypeToIcon[diffType]} />}
      <div className="split-diff-view-file-path-parts">{filePathParts}</div>
      {ctx?.openFile && (
        <Tooltip aria-label={t('Open file')} direction={'sw'} sx={{display: 'flex'}}>
          <IconButton
            size="S"
            variant="invisible"
            className="split-diff-view-file-header-open-button"
            area-label={t('Open file')}
            icon={FileSymlinkFileIcon}
            sx={{opacity: '0.7'}}
            onClick={() => {
              ctx.openFile?.();
            }}
          />
        </Tooltip>
      )}
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
