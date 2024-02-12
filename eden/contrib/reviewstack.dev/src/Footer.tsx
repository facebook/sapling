/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Box, Text} from '@primer/react';
import {useRecoilValue} from 'recoil';
import Link from 'reviewstack/src/Link';
import {primerColorMode} from 'reviewstack/src/themeState';

const logoLinkStyle = {
  opacity: 0.5,
  transition: 'opacity 200ms cubic-bezier(0.08,0.52,0.52,1)',
  ':hover': {
    opacity: 1.0,
  },
};

export default function Footer(): React.ReactElement {
  const colorMode = useRecoilValue(primerColorMode);
  const metaOpenSourceLogo =
    colorMode === 'day' ? '/meta_opensource_logo.svg' : '/meta_opensource_logo_negative.svg';

  const projectLinks = [
    {
      text: 'GitHub Repository',
      href: 'https://github.com/facebook/sapling/tree/main/addons/reviewstack',
    },
    {
      text: 'Sapling SCM',
      href: 'https://sapling-scm.com',
    },
    {
      text: 'Meta Open Source',
      href: 'https://opensource.fb.com',
    },
  ];
  const legalLinks = [
    {text: 'Meta Open Source - Privacy Policy', href: 'https://opensource.fb.com/legal/privacy'},
    {text: 'Meta Open Source - Terms of Use', href: 'https://opensource.fb.com/legal/terms'},
    {
      text: 'GitHub Privacy Statement',
      href: 'https://docs.github.com/en/site-policy/privacy-policies/github-privacy-statement',
    },
    {
      text: 'GitHub Terms of Service',
      href: 'https://docs.github.com/en/site-policy/github-terms/github-terms-of-service',
    },
    {
      text: 'Netlify Privacy Policy',
      href: 'https://www.netlify.com/privacy/',
    },
  ];

  return (
    <Box
      as="footer"
      backgroundColor="canvas.subtle"
      flex="0 0 auto"
      paddingTop={4}
      bottom={0}
      width="100%">
      <Box margin="auto" width={800} paddingTop={2}>
        <Box display="flex" flexDirection="row" justifyContent="space-between">
          <Box>
            <LinkListHeader text="Project Links" />
            {projectLinks.map(props => (
              <LinkCell key={props.href} {...props} />
            ))}
          </Box>
          <Box>
            <LinkListHeader text="Legal" />
            {legalLinks.map(props => (
              <LinkCell key={props.href} {...props} />
            ))}
          </Box>
        </Box>
        <Box textAlign="center">
          <Text sx={logoLinkStyle}>
            <a href="https://opensource.fb.com" target="_blank">
              <img src={metaOpenSourceLogo} width="480px" alt="Meta Open Source Logo" />
            </a>
          </Text>
        </Box>
      </Box>
    </Box>
  );
}

function LinkListHeader({text}: {text: string}): React.ReactElement {
  return (
    <Box paddingBottom={1}>
      <Text fontWeight="bold">{text}</Text>
    </Box>
  );
}

function LinkCell({text, href}: {text: string; href: string}): React.ReactElement {
  return (
    <Box>
      <Link href={href}>{text}</Link>
    </Box>
  );
}
