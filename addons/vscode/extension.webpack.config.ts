/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type webpack from 'webpack';

import CircularDependencyPlugin from 'circular-dependency-plugin';
import path from 'path';

module.exports = {
  target: 'node',
  entry: {extension: './extension/extension.ts'},
  output: {
    filename: '[name].js',
    path: path.resolve(__dirname, 'dist'),
    libraryTarget: 'commonjs2',
    devtoolModuleFilenameTemplate: '../[resource-path]',
  },
  plugins: [
    new CircularDependencyPlugin({
      failOnError: false,
    }),
  ],
  mode: process.env.NODE_ENV ?? 'development',
  devtool: 'source-map',
  module: {
    rules: [
      {
        test: /\.tsx?$/,
        use: [
          {
            loader: 'ts-loader',
            options: {
              compilerOptions: {
                module: 'es2020',
              },
              transpileOnly: true,
              configFile: 'tsconfig.json',
            },
          },
        ],
        exclude: /node_modules/,
      },
    ],
  },
  resolve: {
    extensions: ['.tsx', '.ts', '.js'],
  },
  externals: {ws: '', vscode: 'commonjs vscode'},
} as webpack.Configuration;
