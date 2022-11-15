/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

const {injectAdditionalPlatforms} = require('./customBuildEntry');
const rewire = require('rewire');
const defaults = rewire('react-scripts/scripts/build.js');
const config = defaults.__get__('config');
config.experiments = {
  // Note that `futureDefaults: true` appears to cause problems.
  // From the error message, it seems like it may have to do with
  // https://webpack.js.org/configuration/experiments/#experimentscss,
  // though it is not completely clear.
  asyncWebAssembly: true,
};
config.output.library = 'EdenSmartlog';
config.output.libraryTarget = 'umd';

injectAdditionalPlatforms(config);

// ts-loader is required to reference external typescript projects/files (non-transpiled)
config.module.rules.push({
  test: /\.tsx?$/,
  loader: 'ts-loader',
  exclude: /node_modules/,
  options: {
    transpileOnly: true,
    configFile: 'tsconfig.json',
  },
});

const TerserPlugin = config.optimization.minimizer[0];
// Set this to true to pretty-print the JavaScript for debugging.
TerserPlugin.options.minimizer.options.output.beautify = false;
