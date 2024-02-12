/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export const globalCacheStats = {
  cacheBlobReads: 0,
  cacheTreeReads: 0,
  cacheCommitReads: 0,

  duplicateKeyBlob: 0,
  duplicateKeyTree: 0,
  duplicateKeyCommit: 0,

  gitHubGetBlob: 0,
  gitHubGetTree: 0,
  gitHubGetCommit: 0,
  gitHubGetCommitComparison: 0,
  gitHubGetPullRequest: 0,
  gitHubGetPullRequests: 0,
};

declare global {
  function getReviewStackCacheStats(): typeof globalCacheStats;
  function printReviewStackCacheStats(): void;
}

// Make it so you can run `getReviewStackCacheStats()` in the Console.
globalThis.getReviewStackCacheStats = () => globalCacheStats;

globalThis.printReviewStackCacheStats = () => {
  /* eslint-disable no-console */
  console.log('== IndexedDB reads from cache ==');
  console.log(`blobs: ${globalCacheStats.cacheBlobReads}`);
  console.log(`trees: ${globalCacheStats.cacheTreeReads}`);
  console.log(`commits: ${globalCacheStats.cacheCommitReads}`);

  console.log('== IndexedDB duplicate writes ==');
  console.log(`blobs: ${globalCacheStats.duplicateKeyBlob}`);
  console.log(`trees: ${globalCacheStats.duplicateKeyTree}`);
  console.log(`commits: ${globalCacheStats.duplicateKeyCommit}`);

  console.log('== GitHub API calls ==');
  console.log(`getBlob(): ${globalCacheStats.gitHubGetBlob}`);
  console.log(`getTree(): ${globalCacheStats.gitHubGetTree}`);
  console.log(`getCommit(): ${globalCacheStats.gitHubGetCommit}`);
  console.log(`getCommitComparison(): ${globalCacheStats.gitHubGetCommitComparison}`);
  console.log(`getPullRequest(): ${globalCacheStats.gitHubGetPullRequest}`);
  console.log(`getPullRequests(): ${globalCacheStats.gitHubGetPullRequests}`);
  /* eslint-enable no-console */
};
