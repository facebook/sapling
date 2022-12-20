/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Actor} from './github/pullRequestTimelineTypes';

import ActorAvatar from './ActorAvatar';
import {Box, Text} from '@primer/react';

export default function ActorHeading({actor}: {actor?: Actor | null}): React.ReactElement {
  const login = actor?.login ?? '[unknown]';
  return (
    <Box display="flex" paddingBottom={1} gridGap={1}>
      <ActorAvatar login={actor?.login} url={actor?.avatarUrl} size={24} />
      <Text fontSize={12} fontWeight="bold">
        {login}
      </Text>
    </Box>
  );
}
