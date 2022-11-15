/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Box} from '@primer/react';
import * as React from 'react';

type Props = {
  children: React.ReactNode;
};

/**
 * Renders a horizontal list of items separated by bullets.
 */
export default function BulletItems({children}: Props): React.ReactElement {
  return (
    <Box display="flex" alignItems="center" gridGap={1}>
      {React.Children.toArray(children).map((child, index) => (
        <React.Fragment key={index}>
          {index > 0 && <Box>&bull;</Box>}
          {child}
        </React.Fragment>
      ))}
    </Box>
  );
}
