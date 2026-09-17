/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {ArrowRightIcon, ChevronDownIcon, ChevronRightIcon} from '@primer/octicons-react';
import {Box, Text, Tooltip} from '@primer/react';

export function FileHeader({
  path,
  previousPath,
  open,
  onChangeOpen,
}: {
  path: string;
  previousPath?: string;
  open?: boolean;
  onChangeOpen?: (open: boolean) => void;
}) {
  // Even though the enclosing <SplitDiffView> will have border-radius set, we
  // have to define it again here or things don't look right.
  const color = 'fg.muted';

  const filePathParts = (filePath: string) => (
    <Text fontFamily="mono" fontSize={12} sx={{flexGrow: 1}}>
      {filePath.split('/').reduce((acc, part, idx, pathParts) => {
        // Nest path parts in a particular way so we can use plain CSS
        // hover selectors to underline nested sub-paths.
        const pathSoFar = pathParts.slice(idx).join('/');
        return (
          <span className="file-header-copyable-path" key={idx}>
            {acc}
            <Tooltip
              aria-label={`Copy ${pathSoFar}`}
              direction="se"
              className="file-header-path-element">
              <span
                onClick={() => {
                  navigator.clipboard.writeText(pathSoFar);
                }}>
                {part}
                {idx < pathParts.length - 1 ? '/' : ''}
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
        <Box
          paddingRight={2}
          onClick={() => onChangeOpen(!open)}
          sx={{
            cursor: 'pointer',
            display: 'flex',
          }}>
          {open ? <ChevronDownIcon size={16} /> : <ChevronRightIcon size={16} />}
        </Box>
      )}
      <Box sx={{display: 'flex', flexGrow: 1, alignItems: 'center'}}>
        {previousPath == null ? (
          filePathParts(path)
        ) : (
          <>
            {filePathParts(previousPath)}
            <Box aria-label="moved to" display="flex" marginX={2}>
              <ArrowRightIcon size={16} />
            </Box>
            {filePathParts(path)}
          </>
        )}
      </Box>
    </Box>
  );
}
