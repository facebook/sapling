/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type webpack from 'webpack';

import CircularDependencyPlugin from 'circular-dependency-plugin';
import MiniCssExtractPlugin from 'mini-css-extract-plugin';
import path from 'path';

module.exports = {
  target: 'web',
  // concat preload + actual entry. Preload sets up platform globals before the rest of the app runs.
  entry: {isl: ['./webview/islWebviewPreload.ts', './webview/islWebviewEntry.tsx']},
  output: {
    filename: '[name].js',
    path: path.resolve(__dirname, 'dist', 'webview'),
    devtoolModuleFilenameTemplate: '../[resource-path]',
  },
  mode: process.env.NODE_ENV ?? 'development',
  devtool: 'source-map',
  plugins: [
    new MiniCssExtractPlugin(),
    new CircularDependencyPlugin({
      failOnError: false,
      exclude: /.*node_modules.*/,
    }),
  ],
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
      {
        test: /\.css$/,
        use: [MiniCssExtractPlugin.loader, 'css-loader'],
      },
    ],
  },
  resolve: {
    extensions: ['.tsx', '.ts', '.js'],
  },
  externals: {ws: ''},
} as webpack.Configuration;
