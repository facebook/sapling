/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {FileIcon} from '@primer/octicons-react';
import {Box, Link, Text} from '@primer/react';
import React from 'react';

type Props = {
  onLoadDiff: () => void;
  totalLines: number;
};

const LOAD_DIFF_LINK_SX = {
  cursor: 'pointer',
  fontSize: 14,
  fontWeight: 'bold',
  background: 'none',
  border: 'none',
};

/**
 * Placeholder component shown for large diffs that have not been loaded yet.
 * Displays a "Load Diff" button that the user can click to render the full diff.
 */
function LargeDiffPlaceholder({onLoadDiff, totalLines}: Props): React.ReactElement {
  return (
    <Box
      display="flex"
      flexDirection="column"
      alignItems="center"
      justifyContent="center"
      padding={4}
      bg="canvas.subtle"
      borderTopWidth="1px"
      borderTopStyle="solid"
      borderTopColor="border.default">
      <Box display="flex" alignItems="center" marginBottom={2}>
        <FileIcon size={16} />
      </Box>
      <Link as="button" onClick={onLoadDiff} sx={LOAD_DIFF_LINK_SX}>
        Load Diff
      </Link>
      <Text color="fg.muted" fontSize={12} marginTop={2}>
        Large diffs are not rendered by default.
      </Text>
      <Text color="fg.muted" fontSize={11} marginTop={1}>
        {totalLines.toLocaleString()} lines changed
      </Text>
    </Box>
  );
}

export default React.memo(LargeDiffPlaceholder);
