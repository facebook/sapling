/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Box, Text} from '@primer/react';

export function FileHeader({path}: {path: string}) {
  // Even though the enclosing <SplitDiffView> will have border-radius set, we
  // have to define it again here or things don't look right.
  return (
    <Box
      className="split-diff-view-file-header"
      bg="accent.subtle"
      color="fg.muted"
      paddingX={2}
      paddingY={1}
      lineHeight={2}
      backgroundColor="canvas.subtle"
      borderTopRightRadius={2}
      borderTopLeftRadius={2}
      borderBottomColor="border.default"
      borderBottomStyle="solid"
      borderBottomWidth="1px">
      <Text fontFamily="mono" fontSize={12}>
        {path}
      </Text>
    </Box>
  );
}
