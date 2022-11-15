/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Box, Spinner, Text} from '@primer/react';
import React from 'react';

type Props = {
  message?: string;
};

// eslint-disable-next-line prefer-arrow-callback
export default React.memo(function CenteredSpinner({message}: Props): React.ReactElement {
  const messageEl = message == null ? null : <Text>{message}</Text>;

  return (
    <Box display="flex" alignItems="center" justifyContent="center" padding={2}>
      <Box display="flex" flexDirection="column" alignItems="center" gridGap={1}>
        <Spinner />
        {messageEl}
      </Box>
    </Box>
  );
});
