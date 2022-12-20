/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import URLFor from './URLFor';
import {isConsumerGitHub} from './github/gitHubCredentials';
import {Avatar, Tooltip} from '@primer/react';
import {useCallback, useState} from 'react';
import {useRecoilValue} from 'recoil';

type Props = {
  login?: string;
  size?: number;
  url?: string | null;
};

export default function ActorAvatar({login, size = 24, url}: Props): React.ReactElement {
  const noFallback = useRecoilValue(isConsumerGitHub);
  const src = url ?? URLFor.defaultAvatar();
  // TODO: Note that on GitHub Enterprise, the `src` URL for the avatar may be
  // something like `https://avatars.HOSTNAME/u/ID` and that the browser may
  // reject loading the image in `<img src={src}>`, citing a Cross-Origin Read
  // Blocking (CORB) violation. Chrome directs you to this page to learn more
  // about this security feature:
  //
  // https://chromestatus.com/feature/5629709824032768
  //
  // Unsurprisingly, this causes issues for GHE integrators such as Reviewable,
  // who had the most detailed GitHub issue I could find on the subject:
  //
  // https://github.com/Reviewable/Reviewable/issues/770
  //
  // More references:
  //
  // https://github.com/octokit/octokit.js/discussions/2061
  // https://discourse.drone.io/t/avatars-not-loading-github-enterprise-integration/8168/12
  //
  // Ideally, GitHub would expose a a field to get the base64 contents of the
  // avatar (scaled by size) so the source could be specified as a data URI.
  //
  // We implement the workaround suggested by others, which is to add an error
  // fallback path that creates a "fake avatar" using letters from the user's
  // name/username.
  //
  // We prefer <Avatar> over <AvatarWithFallback> for consumer GitHub since it
  // is more lightweight.
  const avatar = noFallback ? (
    <Avatar src={src} size={size} />
  ) : (
    <AvatarWithFallback login={login} src={src} size={size} />
  );

  if (login == null) {
    return avatar;
  }

  return <Tooltip aria-label={login}>{avatar}</Tooltip>;
}

function AvatarWithFallback({
  login,
  src,
  size,
}: {
  login: string | undefined;
  src: string;
  size: number;
}): React.ReactElement {
  const [hasError, setError] = useState(false);
  const onError = useCallback(() => {
    setError(true);
  }, [setError]);
  if (hasError) {
    const username = login ?? 'unknown';
    // We use two letters from the username to create the key used to pick the
    // color in the array to increase the chance that two users commenting on a
    // thread whose usernames start with the same letter end up with different
    // colors.
    const secondCharCode = username.charCodeAt(1);
    const key = username.charCodeAt(0) * 37 + (isNaN(secondCharCode) ? 13 : secondCharCode);
    const color = fallbackAvatarColors[key % fallbackAvatarColors.length];
    return (
      <div
        style={{
          color: '#fff',
          backgroundColor: color,
          borderRadius: '50%',
          textAlign: 'center',
          height: `${size}px`,
          width: `${size}px`,
        }}>
        {username.charAt(0).toUpperCase()}
      </div>
    );
  } else {
    return (
      <div onError={onError}>
        <Avatar src={src} size={size} />
      </div>
    );
  }
}

// Taken from https://codepen.io/felipepucinelli/pen/QyVJbM.
const fallbackAvatarColors = [
  '#1abc9c',
  '#2ecc71',
  '#3498db',
  '#9b59b6',
  '#34495e',
  '#16a085',
  '#27ae60',
  '#2980b9',
  '#8e44ad',
  '#2c3e50',
  '#f1c40f',
  '#e67e22',
  '#e74c3c',
  '#95a5a6',
  '#f39c12',
  '#d35400',
  '#c0392b',
  '#bdc3c7',
  '#7f8c8d',
];
