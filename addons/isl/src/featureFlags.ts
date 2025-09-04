/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {atom, useAtomValue} from 'jotai';
import {loadable} from 'jotai/utils';
import {randomId} from 'shared/utils';
import serverAPI from './ClientToServerAPI';
import {Internal} from './Internal';
import {atomFamilyWeak, readAtom} from './jotaiUtils';

const bulkFetchedFlagsAtom = atom<Promise<Record<string, boolean>>>(() => {
  if (Internal.featureFlags == null) {
    return Promise.resolve({});
  }
  const knownFlags = Object.values(Internal.featureFlags ?? {});
  return bulkFetchFeatureFlags(knownFlags);
});

/**
 * Boolean values to enable features via remote config.
 * TODO: we could cache values in localstorage to avoid async lookup time if you've previously fetched it
 */
export const featureFlagAsync = atomFamilyWeak((name?: string) => {
  if (name == null) {
    // OSS doesn't have access to feature flags, so they are always "false" by setting the name to null
    return atom(Promise.resolve(false));
  }

  const knownFlags = Object.values(Internal.featureFlags ?? {});
  if (knownFlags.includes(name)) {
    return atom(get => get(bulkFetchedFlagsAtom).then(flags => flags[name]));
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

/** Access featureFlag state without suspending or throwing */
export function useFeatureFlagSync(name: string | undefined) {
  const flag = useAtomValue(featureFlagLoadable(name));
  return flag.state === 'hasData' ? flag.data : false;
}

/** Access featureFlag, suspending if not yet loaded */
export function useFeatureFlagAsync(name: string | undefined) {
  const flag = useAtomValue(featureFlagAsync(name));
  return flag;
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

let featureFlagOverrides: Record<string, boolean> | undefined = undefined;
export const __TEST__ = {
  overrideFeatureFlag: (name: string, value: boolean) => {
    featureFlagOverrides ??= {};
    featureFlagOverrides[name] = value;
  },
  clearFeatureFlagOverrides: () => {
    featureFlagOverrides = undefined;
  },
};

export async function bulkFetchFeatureFlags(
  names: Array<string>,
): Promise<Record<string, boolean>> {
  if (featureFlagOverrides) {
    return Object.fromEntries(names.map(name => [name, featureFlagOverrides?.[name] ?? false]));
  }
  const id = randomId();
  serverAPI.postMessage({
    type: 'bulkFetchFeatureFlags',
    names,
    id,
  });
  const response = await serverAPI.nextMessageMatching(
    'bulkFetchedFeatureFlags',
    message => message.id === id,
  );
  return response.result;
}
