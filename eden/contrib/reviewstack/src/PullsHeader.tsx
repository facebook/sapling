/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Box} from '@primer/react';

type Props = {
  height: number;
};

export default function PullsHeader({height}: Props): React.ReactElement | null {
  return (
    <Box
      height={height}
      borderBottomWidth={1}
      borderBottomStyle="solid"
      borderBottomColor="border.default"
      fontWeight="bold"
      padding={3}>
      Pull Requests
    </Box>
  );
}
