/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @format
 */

import {latestReleaseAssets} from './rawReleaseData';

function findAssetWithFilenameSubstr(searchString: string) {
  for (const asset of latestReleaseAssets.assets) {
    if (asset.name.includes(searchString)) {
      return asset;
    }
  }
  throw new Error(`Releases (rawReleaseData.ts) do not include ${searchString}`);
}

export const latestReleaseVersion = latestReleaseAssets.name;

export const macArmAsset = findAssetWithFilenameSubstr('.bottle.tar.gz');

export const linuxX64Asset = findAssetWithFilenameSubstr('linux-x64.tar.xz');

// Prefix for constructing linux asset url with a variable arch (x64 or arm64).
// e.g. url: `${linuxAssetUrlPrefix}${ARCH}.tar.xz`
export const linuxAssetUrlPrefix = linuxX64Asset.url.replace(/x64\.tar\.xz$/, '');

export const windowsAsset = findAssetWithFilenameSubstr('windows-x64.zip');
