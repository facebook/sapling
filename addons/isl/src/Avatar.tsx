/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DetailedHTMLProps} from 'react';

import * as stylex from '@stylexjs/stylex';
import {useAtomValue} from 'jotai';
import {colors, radius} from '../../components/theme/tokens.stylex';
import serverAPI from './ClientToServerAPI';
import {t} from './i18n';
import {atomFamilyWeak, lazyAtom} from './jotaiUtils';

export const avatarUrl = atomFamilyWeak((author: string) => {
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

export function AvatarImg({
  url,
  username,
  xstyle,
  ...rest
}: {url?: string; username: string; xstyle?: stylex.StyleXStyles} & DetailedHTMLProps<
  React.ImgHTMLAttributes<HTMLImageElement>,
  HTMLImageElement
>) {
  return url == null ? null : (
    <img
      {...stylex.props(styles.circle, xstyle)}
      src={url}
      width={14}
      height={14}
      alt={t("$user's avatar photo", {replace: {$user: username}})}
      {...rest}
    />
  );
}

const styles = stylex.create({
  circle: {
    width: 14,
    height: 14,
    border: '2px solid',
    borderRadius: radius.full,
    borderColor: colors.fg,
  },
  empty: {
    content: '',
    backgroundColor: 'var(--foreground)',
  },
});

export function BlankAvatar() {
  return <div {...stylex.props(styles.circle, styles.empty)} />;
}

export function Avatar({
  username,
  ...rest
}: {username: string} & DetailedHTMLProps<
  React.ImgHTMLAttributes<HTMLImageElement>,
  HTMLImageElement
>) {
  const url = useAtomValue(avatarUrl(username));
  return url == null ? <BlankAvatar /> : <AvatarImg url={url} username={username} {...rest} />;
}

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

// Color palette for initials avatars (12 colors for good distribution)
const AVATAR_COLORS = [
  '#e91e63',
  '#9c27b0',
  '#673ab7',
  '#3f51b5',
  '#2196f3',
  '#00bcd4',
  '#009688',
  '#4caf50',
  '#ff9800',
  '#ff5722',
  '#795548',
  '#607d8b',
];

function hashStringToColor(str: string): string {
  let hash = 0;
  for (let i = 0; i < str.length; i++) {
    // eslint-disable-next-line no-bitwise
    hash = str.charCodeAt(i) + ((hash << 5) - hash);
  }
  return AVATAR_COLORS[Math.abs(hash) % AVATAR_COLORS.length];
}

function getInitials(username: string): string {
  // Extract username from email if present
  const name = username.split('@')[0];
  return name.slice(0, 2).toUpperCase();
}

export function InitialsAvatar({username, size = 20}: {username: string; size?: number}) {
  const initials = getInitials(username);
  const bgColor = hashStringToColor(username);

  return (
    <div
      className="avatar-initials"
      style={{
        backgroundColor: bgColor,
        width: size,
        height: size,
        borderRadius: '50%',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        fontSize: size * 0.5,
        fontWeight: 600,
        color: 'white',
        flexShrink: 0,
      }}
      title={username}>
      {initials}
    </div>
  );
}

export function CommitAvatar({username, size = 20}: {username: string; size?: number}) {
  const url = useAtomValue(avatarUrl(username));

  if (url) {
    return (
      <AvatarImg
        url={url}
        username={username}
        width={size}
        height={size}
        className="commit-author-avatar"
      />
    );
  }

  return <InitialsAvatar username={username} size={size} />;
}
