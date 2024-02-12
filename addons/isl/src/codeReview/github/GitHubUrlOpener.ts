/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {atomFamilyWeak} from '../../jotaiUtils';
import {repositoryInfo} from '../../serverAPIState';
import {atom} from 'jotai';

/**
 * Configured pull request domain to view associated pull requests, such as reviewstack.dev.
 */
export const pullRequestDomain = atom<string | undefined>(get => {
  const info = get(repositoryInfo);
  return info?.type !== 'success' ? undefined : info.pullRequestDomain;
});

export const openerUrlForDiffUrl = atomFamilyWeak((url?: string) => {
  return atom(get => {
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
  });
});
