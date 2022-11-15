/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type webpack from 'webpack';

import path from 'path';

module.exports = {
  entry: {
    child: './proxy/child.ts',
    'run-proxy': './proxy/run-proxy.ts',
    server: './proxy/server.ts',
  },
  output: {
    filename: '[name].js',
    path: path.resolve(__dirname, 'dist'),
  },
  target: 'node',
  mode: process.env.NODE_ENV ?? 'development',
  devtool: 'inline-source-map',
  module: {
    rules: [
      {
        test: /\.tsx?$/,
        use: 'ts-loader',
        exclude: /node_modules/,
      },
    ],
  },
  resolve: {
    extensions: ['.tsx', '.ts', '.js'],
  },
  // ws doesn't play well with webpack, we need to include it separately as a regular node_module
  externals: {ws: 'commonjs ws'},
} as webpack.Configuration;
