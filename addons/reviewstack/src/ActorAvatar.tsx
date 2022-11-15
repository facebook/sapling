/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import URLFor from './URLFor';
import {Avatar, Tooltip} from '@primer/react';

type Props = {
  login?: string;
  size?: number;
  url?: string | null;
};

export default function ActorAvatar({login, size = 24, url}: Props): React.ReactElement {
  const src = url ?? URLFor.defaultAvatar();
  const avatar = <Avatar src={src} size={size} />;

  if (login == null) {
    return avatar;
  }

  return <Tooltip aria-label={login}>{avatar}</Tooltip>;
}
