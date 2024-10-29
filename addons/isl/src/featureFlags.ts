/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import serverAPI from './ClientToServerAPI';
import {atomFamilyWeak, readAtom} from './jotaiUtils';
import {atom, useAtomValue} from 'jotai';
import {loadable} from 'jotai/utils';

/**
 * Boolean values to enable features via remote config.
 * TODO: we could cache values in localstorage to avoid async lookup time if you've previously fetched it
 */
export const featureFlagAsync = atomFamilyWeak((name?: string) => {
  if (name == null) {
    // OSS doesn't have access to feature flags, so they are always "false" by setting the name to null
    return atom(Promise.resolve(false));
  }

  return atom(fetchFeatureFlag(name));
});

export const qeFlagAsync = atomFamilyWeak((name?: string) => {
  if (name == null) {
    // OSS doesn't have access to feature flags, so they are always "false" by setting the name to null
    return atom(Promise.resolve(false));
  }
  return atom(fetchQeFlag(name));
});

const featureFlagLoadable = atomFamilyWeak((name?: string) => {
  return loadable(featureFlagAsync(name));
});

/** Access recoil featureFlag state without suspending or throwing */
export function useFeatureFlagSync(name: string | undefined) {
  const flag = useAtomValue(featureFlagLoadable(name));
  return flag.state === 'hasData' ? flag.data : false;
}

export function getFeatureFlag(name: string | undefined, default_?: boolean): Promise<boolean> {
  if (name == null) {
    return Promise.resolve(default_ ?? false);
  }
  return readAtom(featureFlagAsync(name));
}

export function getQeFlag(name: string | undefined, default_?: boolean): Promise<boolean> {
  if (name == null) {
    return Promise.resolve(default_ ?? false);
  }
  return readAtom(qeFlagAsync(name));
}

async function fetchFeatureFlag(name: string | undefined, default_?: boolean): Promise<boolean> {
  if (name == null) {
    return default_ ?? false;
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
}

async function fetchQeFlag(name: string | undefined, default_?: boolean): Promise<boolean> {
  if (name == null) {
    return default_ ?? false;
  }
  serverAPI.postMessage({
    type: 'fetchQeFlag',
    name,
  });
  const response = await serverAPI.nextMessageMatching(
    'fetchedQeFlag',
    message => message.name === name,
  );
  return response.passes;
}
