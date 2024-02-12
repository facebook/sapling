/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {CommentIcon} from '@primer/octicons-react';
import {Box} from '@primer/react';
import React from 'react';

type Props = {
  count: number;
};

// eslint-disable-next-line prefer-arrow-callback
export default React.memo(function CommentCount({count}: Props): React.ReactElement {
  return (
    <Box>
      <CommentIcon /> {count}
    </Box>
  );
});
