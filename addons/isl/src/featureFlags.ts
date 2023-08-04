/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import serverAPI from './ClientToServerAPI';
import {atomFamily, selectorFamily, useRecoilValueLoadable} from 'recoil';

/**
 * Boolean values to enable features via remote config.
 * TODO: we could cache values in localstorage to avoid async lookup time if you've previously fetched it
 */
export const featureFlag = atomFamily<boolean, string | undefined>({
  key: 'featureFlag',
  default: selectorFamily({
    key: 'featureFlags/default',
    get: (name: string | undefined) => async () => {
      if (name == null) {
        // OSS doesn't have access to feature flags, so they are always "false" by setting the name to null
        return false;
      }
      serverAPI.postMessage({
        type: 'fetchFeatureFlag',
        name,
      });
      const response = await serverAPI.nextMessageMatching(
        'fetchedFeatureFlag',
        message => message.name === name,
      );
      return response.passes;
    },
  }),
});

/** Access recoil featureFlag state without suspending or throwing */
export function useFeatureFlagSync(name: string | undefined) {
  const loadable = useRecoilValueLoadable(featureFlag(name));
  return loadable.valueMaybe() ?? false;
}
