/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffType} from '../patch/parse';
import type {Context} from './types';

import {Icon} from '../Icon';
import {ChevronDownIcon, ChevronUpIcon, FileSymlinkFileIcon} from '@primer/octicons-react';
import {Box, IconButton, Text, Tooltip} from '@primer/react';

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
  const color = diffType === undefined ? 'fg.muted' : diffTypeToColor[diffType];

  const pathSeparator = '/';
  const pathParts = path.split(pathSeparator);

  const t = ctx?.translate ?? (s => s);
  const copy = ctx?.copy;

  const filePathParts = (
    <Text fontFamily="mono" fontSize={12} sx={{flexGrow: 1}}>
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
    </Text>
  );

  return (
    <Box
      className="split-diff-view-file-header"
      display="flex"
      alignItems="center"
      bg="accent.subtle"
      color={color}
      paddingX={2}
      paddingY={1}
      lineHeight={2}
      backgroundColor="canvas.subtle"
      borderTopRightRadius={2}
      borderTopLeftRadius={2}
      borderBottomColor="border.default"
      borderBottomStyle="solid"
      borderBottomWidth="1px">
      {onChangeOpen && (
        <Box paddingRight={2} onClick={() => onChangeOpen(!open)} sx={{cursor: 'pointer'}}>
          {open ? <ChevronUpIcon size={24} /> : <ChevronDownIcon size={24} />}
        </Box>
      )}
      {diffType !== undefined && <Icon icon={diffTypeToIcon[diffType]} />}
      <Box sx={{display: 'flex', flexGrow: 1}}>{filePathParts}</Box>
      {ctx?.openFile && (
        <Tooltip aria-label={t('Open file')} direction={'sw'} sx={{display: 'flex'}}>
          <IconButton
            size="S"
            variant="invisible"
            area-label={t('Open file')}
            icon={FileSymlinkFileIcon}
            sx={{color: 'initial', opacity: '0.7'}}
            onClick={() => {
              ctx.openFile?.();
            }}
          />
        </Tooltip>
      )}
    </Box>
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
