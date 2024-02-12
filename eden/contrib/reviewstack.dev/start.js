/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

const rewire = require('rewire');
const defaults = rewire('react-scripts/scripts/start.js');
const configFactory = defaults.__get__('configFactory');

defaults.__set__('configFactory', env => {
  const config = configFactory(env);
  config.experiments = {
    asyncWebAssembly: true,
  };
  config.externals = {
    react: 'React',
    'react-dom': 'ReactDOM',
    recoil: 'Recoil',
  };
  config.output.library = 'ReviewStack';
  config.module.rules.shift({
    test: /^generated\/textmate\/(.*)\.(json|plist)$/,
    use: [
      {
        loader: 'file-loader',
      },
    ],
  });
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
  return config;
});
