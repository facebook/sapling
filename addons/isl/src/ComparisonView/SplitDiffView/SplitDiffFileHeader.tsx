/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Context} from './types';
import type {EnsureAssignedTogether} from 'shared/EnsureAssignedTogether';
import type {DiffType} from 'shared/patch/parse';

import {Tooltip} from '../../Tooltip';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
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

  const t = ctx?.translate ?? (s => s);
  const copy = ctx?.copy;

  const filePathParts = (
    <>
      {pathParts.reduce((acc, part, idx) => {
        // Nest path parts in a particular way so we can use plain CSS
        // hover selectors to underline nested sub-paths.
        const pathSoFar = pathParts.slice(idx).join(pathSeparator);
        return (
          <span className={'file-header-copyable-path'} key={idx}>
            {acc}
            {
              <Tooltip
                // TODO: better translate API that supports templates.
                component={() => (
                  <span className="file-header-copyable-path-hover">
                    {t('Copy $path').replace('$path', pathSoFar)}
                  </span>
                )}
                delayMs={100}
                placement="bottom">
                <span onClick={copy && (() => copy(pathSoFar))}>
                  {part}
                  {idx < pathParts.length - 1 ? pathSeparator : ''}
                </span>
              </Tooltip>
            }
          </span>
        );
      }, <span />)}
    </>
  );

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
      {ctx?.openFile && (
        <Tooltip title={t('Open file')} placement={'bottom'}>
          <VSCodeButton
            appearance="icon"
            className="split-diff-view-file-header-open-button"
            onClick={() => {
              ctx.openFile?.();
            }}>
            <Icon icon="go-to-file" />
          </VSCodeButton>
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
