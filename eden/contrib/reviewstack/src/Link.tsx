/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Link as PrimerLink, Text} from '@primer/react';

type LinkProps = {
  href: string;
  children: React.ReactNode;
};

let CustomLinkElement: React.FunctionComponent<LinkProps & {style: React.CSSProperties}> | null =
  null;

export function setCustomLinkElement(component: React.FunctionComponent) {
  CustomLinkElement = component;
}

/*
 * Internal Link component that can be overridden at run time, which makes it
 * possible to inject react-router's Link component such that it is a peer
 * dependency rather than a direct dependency. This component should be
 * preferred over @primer/react's Link throughout the app.
 *
 * Note that setCustomLinkElement() should be called before any of the
 * components in this app are rendered if it is going to be used at all.
 */
const Link: React.FunctionComponent<LinkProps> = ({href, children}): React.ReactElement => {
  const isExternalLink = !href.startsWith('/');
  if (CustomLinkElement == null || isExternalLink) {
    return (
      <PrimerLink href={href} target={isExternalLink ? '_blank' : undefined}>
        {children}
      </PrimerLink>
    );
  } else {
    // Note that the <Link> component in Primer is just a vanilla anchor element
    // with additional styling:
    // https://github.dev/primer/react/blob/8ce0eb92d23e2d46760e8b77900e10e7c04da43e/src/Link.tsx
    // We approximate the styling here and can add full support for
    // StyledLinkProps, as necessary.
    const sx = {
      color: 'accent.fg',
      ':hover': {
        textDecoration: 'underline',
      },
    };

    // Note that we use a Primer <Text> with the sx attribute to wrap the
    // children because we need to leverage Primer to ensure :hover is honored.
    // Though it turns out that we cannot override text-decoration in a child
    // of an <a>, so we must also specify the style on CustomLinkElement:
    // https://stackoverflow.com/questions/5434819/cannot-undo-text-decoration-for-child-elements
    return (
      <CustomLinkElement href={href} style={{textDecoration: 'none'}}>
        <Text sx={sx}>{children}</Text>
      </CustomLinkElement>
    );
  }
};

export default Link;
