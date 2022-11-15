/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// this code is called from start.js / build.js react scripts,
// to inject webpack configs to generate additional .html files for different platforms.

const HtmlWebpackPlugin = require('html-webpack-plugin');

const platforms = {
  androidStudio: 'src/platform/androidStudioPlatform.ts',
};

module.exports = {
  injectAdditionalPlatforms(config) {
    const prevEntry = config.entry;
    config.entry = {};
    // inject each platform
    for (const [platform, path] of Object.entries(platforms)) {
      // our goal is to produce:
      // - one main app bundle as usual
      // - one bundle per platform containing only the extra platform code
      // - (via HtmlWebpackPlugin) one html file for main app including only main bundle
      // - (via HtmlWebpackPlugin) one html file per platform including main + platform code
      config.entry[platform] = prevEntry.replace('src/index.tsx', path);
    }
    config.entry.main = prevEntry;
    config.output.filename = 'static/js/[name].js'; // ensure each entry point has a different name

    // replace original HtmlWebpackPlugin, so we can exclude new platform chunks
    config.plugins[0] = new HtmlWebpackPlugin({
      inject: true,
      template: 'public/index.html',
      filename: 'index.html',
      chunks: 'main',
      // "default" browser platform should not include other platform's code
      excludeChunks: [...Object.keys(platforms)],
      ...prodHtmlConfig,
    });
    for (const platform of Object.keys(platforms)) {
      config.plugins.unshift(
        new HtmlWebpackPlugin({
          inject: true,
          template: 'public/index.html',
          filename: `${platform}.html`,
          chunks: [platform, 'main'],
          excludeChunks: [...Object.keys(platforms)].filter(plat => plat !== platform),
          ...prodHtmlConfig,
        }),
      );
    }
  },
};

const prodHtmlConfig =
  process.env.NODE_ENV === 'development'
    ? undefined
    : {
        minify: {
          removeComments: true,
          collapseWhitespace: true,
          removeRedundantAttributes: true,
          useShortDoctype: true,
          removeEmptyAttributes: true,
          removeStyleLinkTypeAttributes: true,
          keepClosingSlash: true,
          minifyJS: true,
          minifyCSS: true,
          minifyURLs: true,
        },
      };
