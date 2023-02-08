/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffType} from '../patch/parse';
import type {Context} from './types';

import {Icon} from '../Icon';
import {Box, Text} from '@primer/react';

export function FileHeader<Id>({
  ctx,
  path,
  diffType,
}: {
  ctx?: Context<Id>;
  path: string;
  diffType?: DiffType;
}) {
  // Even though the enclosing <SplitDiffView> will have border-radius set, we
  // have to define it again here or things don't look right.
  const color = diffType === undefined ? 'fg.muted' : diffTypeToColor[diffType];

  const pathSeparator = '/';
  const pathParts = path.split(pathSeparator);

  return (
    <Box
      className="split-diff-view-file-header"
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
      {diffType !== undefined && <Icon icon={diffTypeToIcon[diffType]} />}
      <Text fontFamily="mono" fontSize={12}>
        {pathParts.reduce((acc, part, idx) => {
          // Nest path parts in a particular way so we can use plain CSS
          // hover selectors to underline nested sub-paths.
          const pathSoFar = pathParts.slice(idx).join(pathSeparator);

          const copy = ctx?.copy;
          const t = ctx?.translate ?? (s => s);

          return (
            <span className={copy && 'file-header-copyable-path'} key={idx}>
              {acc}
              <span
                // TODO: better translate API that supports templates.
                title={copy && t('Copy {path}').replace('{path}', pathSoFar)}
                onClick={copy && (() => copy(pathSoFar))}
                className={'file-header-path-element'}>
                {part}
                {idx < pathParts.length - 1 ? pathSeparator : ''}
              </span>
            </span>
          );
        }, <span />)}
      </Text>
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
