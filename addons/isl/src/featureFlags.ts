/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import serverAPI from './ClientToServerAPI';
import {atomFamilyWeak, lazyAtom} from './jotaiUtils';
import {atom, useAtomValue} from 'jotai';

/**
 * Boolean values to enable features via remote config.
 * TODO: we could cache values in localstorage to avoid async lookup time if you've previously fetched it
 */
export const featureFlag = atomFamilyWeak((name?: string) => {
  if (name == null) {
    // OSS doesn't have access to feature flags, so they are always "false" by setting the name to null
    return atom(false);
  }

  return lazyAtom(async () => {
    serverAPI.postMessage({
      type: 'fetchFeatureFlag',
      name,
    });
    const response = await serverAPI.nextMessageMatching(
      'fetchedFeatureFlag',
      message => message.name === name,
    );
    return response.passes;
  }, undefined);
});

/** Access recoil featureFlag state without suspending or throwing */
export function useFeatureFlagSync(name: string | undefined) {
  const maybe = useAtomValue(featureFlag(name));
  return maybe ?? false;
}
