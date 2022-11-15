/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {repositoryInfo} from '../CodeReviewInfo';
import {selector, selectorFamily} from 'recoil';

/**
 * Configured pull request domain to view associated pull requests, such as reviewstack.dev.
 */
export const pullRequestDomain = selector<string | undefined>({
  key: 'pullRequestDomain',
  get: ({get}) => {
    const info = get(repositoryInfo);
    return info?.pullRequestDomain;
  },
});

export const openerUrlForDiffUrl = selectorFamily<string | undefined, string | undefined>({
  key: 'openerUrlForDiffUrl',
  get:
    (url?: string) =>
    ({get}) => {
      if (!url) {
        return url;
      }
      const newDomain = get(pullRequestDomain);
      if (newDomain) {
        return url.replace(
          /^https:\/\/[^/]+/,
          newDomain.startsWith('https://') ? newDomain : `https://${newDomain}`,
        );
      }
      return url;
    },
});
