/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Text} from '@primer/react';

/**
 * There does not appear to be an official Primer equivalent to <code> in the
 * public API, but this appears to approximate what is used on github.com.
 */
export default function InlineCode({children}: {children: React.ReactNode}): React.ReactElement {
  return (
    <Text
      bg="neutral.muted"
      sx={{borderRadius: '6px'}}
      fontFamily="ui-monospace, SFMono-Regular, SF Mono, Menlo, Consolas, Liberation Mono, monospace"
      fontSize="85%"
      padding="0.2em 0.4em"
      lineHeight="1.5">
      {children}
    </Text>
  );
}
