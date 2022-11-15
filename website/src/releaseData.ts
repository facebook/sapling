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

export const macArmAsset = findAssetWithFilenameSubstr(
  'arm64_monterey.bottle.tar.gz',
);

export const macIntelAsset = findAssetWithFilenameSubstr(
  '.monterey.bottle.tar.gz',
);

export const windowsAsset = findAssetWithFilenameSubstr('sapling_windows');
