/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {T} from '../i18n';
import platform from '../platform';
import {themeState} from '../theme';
import {Ribbon} from './Ribbon';
import {VSCodeLink, VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useRecoilValue} from 'recoil';
import {Icon} from 'shared/Icon';

export function DismissButton({dismiss}: {dismiss: () => void}) {
  return (
    <div className="dismiss">
      <VSCodeButton appearance="icon" onClick={dismiss}>
        <Icon icon="x" />
      </VSCodeButton>
    </div>
  );
}

export function Link({
  children,
  href,
  className,
  onNavigate,
}: {
  children: React.ReactNode;
  href: string;
  className?: string;
  onNavigate?: () => unknown;
}) {
  return (
    <VSCodeLink
      className={className}
      onClick={() => {
        onNavigate?.();
        platform.openExternalLink(href);
      }}>
      {children}
    </VSCodeLink>
  );
}

export function Subtitle({children}: {children: React.ReactNode}) {
  return <h2 className="subtitle">{children}</h2>;
}

export function Squares({children}: {children: React.ReactNode}) {
  return <div className="squares">{children}</div>;
}

export function SquareLink({
  children,
  href,
  onNavigate,
}: {
  children: React.ReactNode;
  href: string;
  onNavigate?: () => unknown;
}) {
  return (
    <a
      className="square"
      tabIndex={0}
      onKeyDown={e => {
        if (e.key === 'Enter') {
          platform.openExternalLink(href);
          e.preventDefault();
          onNavigate?.();
        }
      }}
      onClick={e => {
        platform.openExternalLink(href);
        e.preventDefault();
        onNavigate?.();
      }}>
      {children}
    </a>
  );
}

export function Card({
  title,
  imgDark,
  imgLight,
  alt,
  description,
  side,
  comingSoon,
}: {
  title: React.ReactNode;
  imgLight: string;
  imgDark: string;
  alt: string;
  description: React.ReactNode;
  side: 'left' | 'right';
  comingSoon?: boolean;
}) {
  const theme = useRecoilValue(themeState);
  const imgEl = (
    <img src={theme === 'light' ? imgLight : imgDark} alt={alt} className="card-image" />
  );
  return (
    <div className="card">
      {side === 'left' && imgEl}
      <div className="card-details">
        <div className="card-title">{title}</div>
        <div className="card-description">
          <p>{description}</p>
        </div>
      </div>
      {side === 'right' && imgEl}
      {comingSoon ? (
        <Ribbon>
          <T>Coming Soon!</T>
        </Ribbon>
      ) : null}
    </div>
  );
}

export function Callout({
  title,
  imgDark,
  imgLight,
  alt,
}: {
  title: React.ReactNode;
  imgLight: string;
  imgDark: string;
  alt: string;
}) {
  const theme = useRecoilValue(themeState);
  return (
    <div className="callout">
      <img src={theme === 'light' ? imgLight : imgDark} alt={alt} className="callout-image" />
      <span>{title}</span>
    </div>
  );
}

export function CallToAction({children}: {children: React.ReactNode}) {
  return (
    <div className="call-to-action">
      <Icon icon="alert" size="L" />
      <p>{children}</p>
    </div>
  );
}
