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
  return null;
}

export const latestReleaseVersion = latestReleaseAssets.name;

export const macArmAsset = findAssetWithFilenameSubstr('arm64_monterey.bottle.tar.gz');

export const macIntelAsset = findAssetWithFilenameSubstr('.monterey.bottle.tar.gz');

export const ubuntu20 = findAssetWithFilenameSubstr('Ubuntu20.04.deb');

export const ubuntu22 = findAssetWithFilenameSubstr('Ubuntu22.04.deb');

export const windowsAsset = findAssetWithFilenameSubstr('sapling_windows');
