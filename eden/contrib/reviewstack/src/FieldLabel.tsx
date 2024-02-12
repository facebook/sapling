/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Box, Text} from '@primer/react';
import React from 'react';

type Props = {
  label: string;
};

// eslint-disable-next-line prefer-arrow-callback
export default React.memo(function FieldLabel({label}: Props): React.ReactElement {
  return (
    <Box display="flex" alignItems="center" justifyContent="center" padding={1}>
      <Text fontSize={1} fontWeight="bold">
        {label}
      </Text>
    </Box>
  );
});
