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
