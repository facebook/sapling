/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepoInfo} from './types';

import {atom} from 'jotai';
import serverAPI from './ClientToServerAPI';
import {writeAtom} from './jotaiUtils';
import {repositoryInfo} from './serverAPIState';
import {registerDisposable} from './utils';

/**
 * Hidden master branch config fetched from sitevar.
 * Maps OD type to list of repo paths where master should be hidden.
 * Example: {'instagram_www': ['fbsource/fbcode/instagram-server', 'fbsource/www']}
 */
export const hiddenMasterBranchConfigAtom = atom<Record<string, Array<string>> | null>(null);

/**
 * OD type fetched once on startup
 */
const odTypeAtom = atom<string | null>(null);

/**
 * Current working directory from server
 */
const cwdAtom = atom<string>('');

/**
 * Computed atom that determines if master branch should be hidden
 * based on the fetched config, OD type, and current repo path.
 */
export const shouldHideMasterAtom = atom(get => {
  const config = get(hiddenMasterBranchConfigAtom);
  const odType = get(odTypeAtom);
  const cwd = get(cwdAtom);
  const repoInfo = get(repositoryInfo);

  if (!repoInfo) {
    return false;
  }

  return checkShouldHideMaster(config, odType, cwd, repoInfo);
});

/**
 * Computed atom that indicates if the hidden master feature is available.
 * This is true when the sitevar config has been fetched and the current OD type
 * is enabled in the config.
 */
export const hiddenMasterFeatureAvailableAtom = atom(get => {
  const config = get(hiddenMasterBranchConfigAtom);
  const odType = get(odTypeAtom);
  // Feature is available if config exists and current OD type is in the config
  return config != null && odType != null && odType in config;
});

/**
 * Check if master branch should be hidden based on sitevar config and repo path.
 */
function checkShouldHideMaster(
  hiddenMasterBranchConfig: Record<string, Array<string>> | null,
  odType: string | null,
  cwd: string,
  repoInfo: RepoInfo,
): boolean {
  if (!hiddenMasterBranchConfig || !odType) {
    return false;
  }

  if (repoInfo.type !== 'success') {
    return false;
  }

  const repoPaths = hiddenMasterBranchConfig[odType];
  if (!repoPaths) {
    return false;
  }

  const repoRoot = repoInfo.repoRoot;

  // Check if current working directory matches any of the configured paths
  const shouldHide = repoPaths.some(configPath => {
    // Strip 'fbsource/' prefix if present to get the relative path
    const relativePath = configPath.startsWith('fbsource/')
      ? configPath.substring('fbsource/'.length)
      : configPath;

    // Construct the full expected path
    const fullPath = `${repoRoot}/${relativePath}`;

    // Check if current working directory matches the configured path
    return cwd === fullPath || cwd.startsWith(`${fullPath}/`);
  });

  return shouldHide;
}

// Listen for config from server and store it
registerDisposable(
  serverAPI,
  serverAPI.onMessageOfType('fetchedHiddenMasterBranchConfig', data => {
    // Store the config, OD type, and cwd in atoms for quick access
    writeAtom(hiddenMasterBranchConfigAtom, data.config || {});
    writeAtom(odTypeAtom, data.odType || '');
    writeAtom(cwdAtom, data.cwd);
  }),
  import.meta.hot,
);
