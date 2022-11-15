/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @format
 */

// @ts-check
// Note: type annotations allow type checking and IDEs autocompletion

const {fbContent} = require('docusaurus-plugin-internaldocs-fb/internal');

// Sapling specific constants
const {gitHubRepo, gitHubRepoName} = require('./constants');

// Footer URLS
const twitter = 'https://twitter.com/MetaOpenSource';
const openSourceWebsite = 'https://opensource.fb.com/';
const watchmanRepo = 'https://github.com/facebook/watchman';
const ghstackRepo = 'https://github.com/ezyang/ghstack';
const vsCodeRepo = 'https://github.com/facebookexperimental/fb-vscode';

/** @type {import('@docusaurus/types').Config} */
const config = {
  title: 'Sapling',
  tagline: 'A Scalable, User-Friendly Source Control System',
  url: 'https://sapling-scm.com',
  baseUrl: '/',
  onBrokenLinks: 'throw',
  onBrokenMarkdownLinks: 'warn',
  favicon: 'img/Sapling_favicon-light-green-transparent-big.svg',

  // GitHub pages deployment config.
  // If you aren't using GitHub pages, you don't need these.
  organizationName: 'facebook',
  projectName: gitHubRepoName,

  presets: [
    [
      require.resolve('docusaurus-plugin-internaldocs-fb/docusaurus-preset'),
      /** @type {import('@docusaurus/preset-classic').Options} */
      {
        docs: {
          sidebarPath: require.resolve('./sidebars.js'),
          editUrl: fbContent({
            internal:
              'https://www.internalfb.com/code/fbsource/fbcode/eden/website/',
            // This does not exist yet...
            external:
              'https://github.com/facebookexperimental/eden/tree/main/website',
          }),
          remarkPlugins: [require('sapling-output-plugin')],
        },
        staticDocsProject: 'sapling',
        trackingFile: 'xplat/staticdocs/WATCHED_FILES',
        'remark-code-snippets': {
          baseDir: '..',
        },
        enableEditor: true,
        theme: {
          customCss: require.resolve('./src/css/custom.css'),
        },
      },
    ],
  ],
  customFields: {
    fbRepoName: 'fbsource',
    ossRepoPath: 'fbcode/eden',
  },

  themeConfig:
    /** @type {import('@docusaurus/preset-classic').ThemeConfig} */
    {
      navbar: {
        title: 'Sapling',
        logo: {
          alt: 'Sapling Logo',
          src: 'img/Sapling_icon-dark-green.svg',
        },
        items: [
          // Please keep GitHub link to the right for consistency.
          {
            href: gitHubRepo,
            label: 'GitHub',
            position: 'right',
          },
        ],
      },
      footer: {
        style: 'dark',
        links: [
          {
            title: 'Useful Links',
            items: [
              {
                label: 'GitHub',
                href: gitHubRepo,
              },
              {
                label: 'Twitter',
                href: twitter,
              },
              {
                label: 'Meta Open Source',
                href: openSourceWebsite,
              },
            ],
          },
          {
            title: 'Related Projects',
            items: [
              {
                label: 'ghstack',
                href: ghstackRepo,
              },
              {
                label: 'Watchman',
                href: watchmanRepo,
              },
              {
                // TODO: Consider changing this to Rocksdb or Buck2 in the future
                label: 'VS Code @ Meta',
                href: vsCodeRepo,
              },
            ],
          },
          {
            title: 'Legal',
            // Please do not remove the privacy and terms, it's a legal requirement.
            items: [
              {
                label: 'Privacy',
                href: 'https://opensource.fb.com/legal/privacy/',
              },
              {
                label: 'Terms',
                href: 'https://opensource.fb.com/legal/terms/',
              },
              {
                label: 'Data Policy',
                href: 'https://opensource.fb.com/legal/data-policy/',
              },
              {
                label: 'Cookie Policy',
                href: 'https://opensource.fb.com/legal/cookie-policy/',
              },
            ],
          },
        ],
        logo: {
          alt: 'Meta Open Source Logo',
          // This default includes a positive & negative version, allowing for
          // appropriate use depending on your site's style.
          src: '/img/meta_opensource_logo_negative.svg',
          href: 'https://opensource.fb.com',
        },
        // Please do not remove the credits, help to publicize Docusaurus :)
        copyright: `Copyright © ${new Date().getFullYear()} Meta Platforms, Inc. Built with Docusaurus.`,
      },
    },

  plugins: [
    async function webpack_config(context, options) {
      return {
        name: 'webpack-config',
        configureWebpack(config, isServer, utils, content) {
          return {
            experiments: {
              asyncWebAssembly: true,
            }
          };
        },
      };
    },
  ],
};

module.exports = config;
