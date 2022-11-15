/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @format
 */

// This file is in JavaScript using CommonJS exports rather than TypeScript with
// ES Modules so it can be imported in docusaurus.config.js.

const gitHubRepoName = 'sapling';

const gitHubRepo = `https://github.com/facebook/${gitHubRepoName}`;

/**
 * Note that `path` should not start with a slash.
 * @return string
 */
function gitHubRepoUrl(path /* string */) {
  return `${gitHubRepo}/${path}`;
}

const latestReleasePage = gitHubRepoUrl('releases/latest');

module.exports = {
  gitHubRepoName,
  gitHubRepo,
  gitHubRepoUrl,
  latestReleasePage,
};
