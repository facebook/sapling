/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import serverAPI from './ClientToServerAPI';
import {atomFamilyWeak, lazyAtom} from './jotaiUtils';
import {useAtomValue} from 'jotai';

const avatarUrl = atomFamilyWeak((author: string) => {
  // Rate limitor for the same author is by lazyAtom and atomFamilyWeak caching.
  return lazyAtom(async () => {
    serverAPI.postMessage({
      type: 'fetchAvatars',
      authors: [author],
    });
    const result = await serverAPI.nextMessageMatching('fetchedAvatars', ({authors}) =>
      authors.includes(author),
    );
    return result.avatars.get(author);
  }, undefined);
});

/** Render as a SVG pattern */
export function AvatarPattern({
  username,
  size,
  id,
  fallbackFill,
}: {
  username: string;
  size: number;
  id: string;
  fallbackFill: string;
}) {
  const img = useAtomValue(avatarUrl(username));
  return (
    <pattern
      id={id}
      patternUnits="userSpaceOnUse"
      width={size}
      height={size}
      x={-size / 2}
      y={-size / 2}>
      <rect width={size} height={size} fill={fallbackFill} strokeWidth={0} />
      <image href={img} width={size} height={size} />
    </pattern>
  );
}
